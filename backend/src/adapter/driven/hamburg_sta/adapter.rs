//! The Hamburg SensorThings [`DataProvider`] adapter: configuration, discovery
//! and measurement serving.
//!
//! Discovery is cheap: one paginated `Datastreams` query filtered by
//! `properties/layerName eq 'Anzahl_Fahrraeder_Zaehlfeld_5-Min'` (with
//! `$expand=Thing`), which yields the live `Zählfeld` datastreams *and* the
//! legacy `(veraltet)` ones that extend each field's history. The parsed index
//! is cached for `cache_duration` seconds.
//!
//! Observations are read through the source-level
//! [`DataProvider::get_measurements_source`]: each field (channel) owns two
//! **independent** stream readers (legacy + current), each with its own buffer
//! and `@iot.nextLink` continuation. Because the streams advance on their own
//! cursors, a long legacy backfill never re-downloads the current feed, and the
//! legacy + current rows are merged per field (dedup keep-last, current wins).
//! Whole pages are buffered (no partial-page re-fetch) and up to `concurrency`
//! fields are paged in parallel per batch.

use std::collections::{HashMap, HashSet, VecDeque};
use std::net::TcpStream;
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use chrono::{DateTime, Utc};

use crate::adapter::driven::source_merge::{ChannelPage, SourceScanner};
use crate::core::domain::configuration::configuration::value_objects::DataSourceConfiguration;
use crate::core::domain::configuration::error::ConfigError;
use crate::core::domain::data_source::provider_message::ProviderMessageSeverity;
use crate::core::domain::data_source::provider_port::{
    ChannelRecord, CountingStationRecord, DataProvider, MeasurementRecord, ProviderError,
    ProviderMessageSink, SourceMeasurement, SourceMeasurementBatch,
};
use crate::core::domain::health::HealthStatus;

use super::fetcher::{HttpResourceFetcher, ResourceFetcher};
use super::parsing::{
    HamburgIndex, LAYER_NAME, RawDatastream, RawObservation, RawPage, build_index,
    observations_url, parse_observation,
};

pub(crate) const PROVIDER_TYPE: &str = "hamburg_sta_http_provider";
pub(crate) const DEFAULT_BASE_URL: &str = "https://iot.hamburg.de/v1.0/";
pub(crate) const DEFAULT_MAX_MEASUREMENT_BATCH_SIZE: usize = 1000;
pub(crate) const DEFAULT_CACHE_DURATION_SECS: u64 = 300;
const OBSERVATIONS_PAGE_SIZE: usize = 1000;
const DISCOVERY_PAGE_SIZE: usize = 500;
/// Fields paged concurrently per source-level batch.
const DEFAULT_CONCURRENCY: usize = 8;

pub struct HamburgStaAdapter {
    base_url: String,
    max_measurement_batch_size: usize,
    cache_duration: Duration,
    include_legacy: bool,
    /// Fields paged in parallel per [`Self::get_measurements_source`] batch.
    concurrency: usize,
    fetcher: Arc<dyn ResourceFetcher>,
    /// Scoped provider-message sink, attached by `StartupService`. `None` until
    /// attached.
    messages: Mutex<Option<Arc<dyn ProviderMessageSink + Send + Sync>>>,
    /// In-memory cache of the parsed index.
    cache: Mutex<Option<CachedData>>,
    /// Serializes cache refresh across threads.
    refresh_lock: Mutex<()>,
    /// Whole-source (channel-interleaved) reader state for the current run.
    scanner: Mutex<Option<SourceScanner>>,
    /// Per-field (channel) stream readers for the current run.
    readers: Mutex<Option<ReaderSet>>,
}

#[derive(Clone)]
struct CachedData {
    fetched_at: Instant,
    index: Arc<HamburgIndex>,
}

/// One SensorThings observation stream (legacy or current) of a field.
///
/// A stream keeps an independent cursor: only its own `@iot.nextLink` advances
/// it, so paging one feed never re-downloads the other. `pending` is the head
/// row ready for the merge; `buffered` holds already-fetched rows past it.
struct StreamReader {
    /// The SensorThings datastream id (used for diagnostics).
    datastream_id: i64,
    /// First-page URL, built once per run from the anchor watermark.
    initial_url: String,
    /// The next candidate row (fetched, not yet emitted).
    pending: Option<MeasurementRecord>,
    /// Fetched rows beyond `pending`, ascending.
    buffered: VecDeque<MeasurementRecord>,
    /// Server continuation URL; `None` once the stream is exhausted.
    next_link: Option<String>,
    exhausted: bool,
}

impl StreamReader {
    fn new(datastream_id: i64, initial_url: String) -> Self {
        Self {
            datastream_id,
            initial_url,
            pending: None,
            buffered: VecDeque::new(),
            next_link: None,
            exhausted: false,
        }
    }

    /// Ensures a candidate row (`pending`) is set whenever the stream still has
    /// data, fetching a whole page on demand (the `@iot.nextLink` continuation,
    /// or the initial URL once per run) and buffering it.
    fn ensure_pending(&mut self, fetcher: &dyn ResourceFetcher) -> Result<(), ProviderError> {
        while self.pending.is_none() && !self.exhausted {
            let url = match self.next_link.take() {
                Some(link) => link,
                None => self.initial_url.clone(),
            };
            let json = fetcher.fetch(&url).map_err(|message| {
                ProviderError::Unreachable(format!(
                    "observations fetch failed (datastream {}): {message}",
                    self.datastream_id
                ))
            })?;
            let page: RawPage<RawObservation> = serde_json::from_str(&json).map_err(|e| {
                ProviderError::InvalidData(format!(
                    "invalid Observations json (datastream {}): {e}",
                    self.datastream_id
                ))
            })?;
            for observation in page.value {
                if let Some(record) = parse_observation(&observation) {
                    self.buffered.push_back(record);
                }
            }
            self.next_link = page.next_link;
            if self.next_link.is_none() {
                self.exhausted = true;
            }
            self.pending = self.buffered.pop_front();
        }
        Ok(())
    }

    /// Removes and returns the head row, promoting the next buffered row (if
    /// any) to `pending`.
    fn pop(&mut self) -> Option<MeasurementRecord> {
        let row = self.pending.take();
        self.pending = self.buffered.pop_front();
        row
    }

    /// Whether the stream can still produce a row.
    fn has_more(&self) -> bool {
        self.pending.is_some() || !self.buffered.is_empty() || !self.exhausted
    }
}

/// The legacy + current readers of one field (a channel's `Zählfeld`).
struct FieldReader {
    /// Reader of the `(veraltet)` history stream, when enabled and present.
    legacy: Option<StreamReader>,
    /// Reader of the live field stream.
    current: StreamReader,
}

/// The adapter's per-field readers for the current run, anchored at `from`.
struct ReaderSet {
    anchor: Option<DateTime<Utc>>,
    ids: Vec<String>,
    /// Per-field readers (a per-field lock keeps concurrent batches disjoint).
    fields: HashMap<String, Arc<Mutex<FieldReader>>>,
}

impl HamburgStaAdapter {
    pub fn provider_type() -> &'static str {
        PROVIDER_TYPE
    }

    /// Builds the adapter from the data source's provider vars.
    ///
    /// Optional vars: `base_url` (default the official Hamburg SensorThings
    /// root), `max_measurement_batch_size` (default `1000`), `cache_duration`
    /// (seconds, default `300`), `include_legacy` (default `true`, merges the
    /// `(veraltet)` field series for history), `concurrency` (default `8`,
    /// fields paged in parallel). A missing/invalid value is a configuration
    /// error (blocks startup).
    pub fn new(config: &DataSourceConfiguration) -> Result<Self, ConfigError> {
        Self::with_fetcher(config, Arc::new(HttpResourceFetcher))
    }

    pub(crate) fn with_fetcher(
        config: &DataSourceConfiguration,
        fetcher: Arc<dyn ResourceFetcher>,
    ) -> Result<Self, ConfigError> {
        let base_url = match config.provider().var("base_url") {
            Some(raw) => raw.to_string(),
            None => DEFAULT_BASE_URL.to_string(),
        };
        let base_url = if base_url.ends_with('/') {
            base_url
        } else {
            format!("{base_url}/")
        };

        let max_measurement_batch_size = match config.provider().var("max_measurement_batch_size") {
            Some(raw) => raw.parse::<usize>().map_err(|_| {
                ConfigError::InvalidFormat(format!(
                    "{PROVIDER_TYPE}: var 'max_measurement_batch_size' is not a valid number"
                ))
            })?,
            None => DEFAULT_MAX_MEASUREMENT_BATCH_SIZE,
        };

        let cache_duration = match config.provider().var("cache_duration") {
            Some(raw) => raw.parse::<u64>().map_err(|_| {
                ConfigError::InvalidFormat(format!(
                    "{PROVIDER_TYPE}: var 'cache_duration' is not a valid number"
                ))
            })?,
            None => DEFAULT_CACHE_DURATION_SECS,
        };

        let include_legacy = match config.provider().var("include_legacy") {
            Some(raw) => raw.parse::<bool>().map_err(|_| {
                ConfigError::InvalidFormat(format!(
                    "{PROVIDER_TYPE}: var 'include_legacy' is not a valid boolean"
                ))
            })?,
            None => true,
        };

        let concurrency = match config.provider().var("concurrency") {
            Some(raw) => raw.parse::<usize>().map_err(|_| {
                ConfigError::InvalidFormat(format!(
                    "{PROVIDER_TYPE}: var 'concurrency' is not a valid number"
                ))
            })?,
            None => DEFAULT_CONCURRENCY,
        }
        .max(1);

        Ok(Self {
            base_url,
            max_measurement_batch_size,
            cache_duration: Duration::from_secs(cache_duration),
            include_legacy,
            concurrency,
            fetcher,
            messages: Mutex::new(None),
            cache: Mutex::new(None),
            refresh_lock: Mutex::new(()),
            scanner: Mutex::new(None),
            readers: Mutex::new(None),
        })
    }

    /// The configured cache window in seconds.
    pub fn cache_duration_secs(&self) -> u64 {
        self.cache_duration.as_secs()
    }

    /// The configured number of fields paged in parallel per batch.
    pub(crate) fn concurrency(&self) -> usize {
        self.concurrency
    }

    /// The attached provider-message sink, if any (cloned handle for callers).
    fn messages_sink(&self) -> Option<Arc<dyn ProviderMessageSink + Send + Sync>> {
        self.messages.lock().unwrap().clone()
    }

    /// Emits a scoped provider message (best-effort).
    pub(crate) fn emit(&self, severity: ProviderMessageSeverity, message: impl AsRef<str>) {
        if let Some(sink) = self.messages.lock().unwrap().as_ref() {
            let _ = sink.provider_event_occurred(severity, message.as_ref());
        }
    }

    /// Ensures a usable parsed index exists and returns it.
    fn ensure_index(&self) -> Result<Arc<HamburgIndex>, ProviderError> {
        if let Some(cached) = self.cache.lock().unwrap().as_ref()
            && cached.fetched_at.elapsed() < self.cache_duration
        {
            return Ok(cached.index.clone());
        }
        let _guard = self
            .refresh_lock
            .lock()
            .map_err(|_| ProviderError::Storage("refresh lock poisoned".to_string()))?;
        if let Some(cached) = self.cache.lock().unwrap().as_ref()
            && cached.fetched_at.elapsed() < self.cache_duration
        {
            return Ok(cached.index.clone());
        }

        let datastreams = self.discover_datastreams()?;
        let messages = self.messages_sink();
        let index = Arc::new(build_index(&datastreams, messages.as_deref()));
        self.emit(
            ProviderMessageSeverity::Info,
            format!(
                "hamburg discovery refreshed: {} stations, {} channels, {} fields",
                index.stations.len(),
                index.channels.len(),
                index.fields.len(),
            ),
        );
        *self.cache.lock().unwrap() = Some(CachedData {
            fetched_at: Instant::now(),
            index: index.clone(),
        });
        Ok(index)
    }

    /// Fetches every field 5-min datastream (current + legacy), following
    /// `@iot.nextLink` pages.
    fn discover_datastreams(&self) -> Result<Vec<RawDatastream>, ProviderError> {
        let filter = filter_param(&format!("properties/layerName eq '{LAYER_NAME}'"));
        let mut url = format!(
            "{}Datastreams?$filter={}&$top={DISCOVERY_PAGE_SIZE}&\
             $select=@iot.id,name,properties,observedArea&$expand=Thing($select=properties)",
            self.base_url, filter,
        );
        let mut all = Vec::new();
        loop {
            let json = self.fetcher.fetch(&url).map_err(|message| {
                ProviderError::Unreachable(format!("datastream discovery failed: {message}"))
            })?;
            let page: RawPage<RawDatastream> = serde_json::from_str(&json).map_err(|e| {
                ProviderError::InvalidData(format!("invalid Datastreams json: {e}"))
            })?;
            all.extend(page.value);
            match page.next_link {
                Some(next) => url = next,
                None => break,
            }
        }
        Ok(all)
    }

    /// Builds the paged observations URL for one datastream, filtered to the
    /// watermark window.
    fn observations_url(
        &self,
        datastream_id: i64,
        from: Option<DateTime<Utc>>,
        to: Option<DateTime<Utc>>,
    ) -> String {
        let base = observations_url(&self.base_url, datastream_id);
        let mut filter = String::new();
        if let Some(from) = from {
            filter.push_str(&format!("phenomenonTime ge {}", from.to_rfc3339()));
        }
        if let Some(to) = to {
            if !filter.is_empty() {
                filter.push_str(" and ");
            }
            filter.push_str(&format!("phenomenonTime le {}", to.to_rfc3339()));
        }
        // `$orderby=phenomenonTime asc` must be percent-encoded (`%20`): ureq 3
        // parses the URL with `http::Uri`, which rejects a literal space in the
        // query string ("http: invalid uri character"), unlike ureq 2.
        const ORDERBY: &str = "$orderby=phenomenonTime%20asc";
        if filter.is_empty() {
            format!("{base}?{ORDERBY}&$top={OBSERVATIONS_PAGE_SIZE}")
        } else {
            format!(
                "{base}?$filter={}&{ORDERBY}&$top={OBSERVATIONS_PAGE_SIZE}",
                filter_param(&filter),
            )
        }
    }

    /// (Re)seeds the per-field stream readers when the run anchor or the
    /// channel set changes. The readers are anchored at `from` (the persisted
    /// watermark) so a resumed run starts at its checkpoint.
    fn ensure_readers(&self, from: Option<DateTime<Utc>>, ids: &[String], index: &HamburgIndex) {
        let mut guard = self.readers.lock().unwrap();
        let needs_seed = match guard.as_ref() {
            Some(set) => set.anchor != from || set.ids.as_slice() != ids,
            None => true,
        };
        if !needs_seed {
            return;
        }

        let mut fields = HashMap::with_capacity(ids.len());
        for id in ids {
            let Some(sources) = index.fields.get(id) else {
                continue;
            };
            let Some(current_id) = sources.current else {
                continue;
            };
            let mut legacy = None;
            if self.include_legacy
                && let Some(legacy_id) = sources.legacy
            {
                legacy = Some(StreamReader::new(
                    legacy_id,
                    self.observations_url(legacy_id, from, None),
                ));
            }
            fields.insert(
                id.clone(),
                Arc::new(Mutex::new(FieldReader {
                    current: StreamReader::new(
                        current_id,
                        self.observations_url(current_id, from, None),
                    ),
                    legacy,
                })),
            );
        }
        *guard = Some(ReaderSet {
            anchor: from,
            ids: ids.to_vec(),
            fields,
        });
    }

    /// Serves the next page of one field (channel): a streaming merge of its
    /// legacy + current readers (dedup keep-last, current wins), ascending,
    /// returning up to `budget` rows. `anchor` is the run watermark; rows at or
    /// before it (only the boundary row) are never re-emitted.
    fn fill_page(
        &self,
        external_id: &str,
        budget: usize,
        anchor: Option<DateTime<Utc>>,
    ) -> Result<ChannelPage, ProviderError> {
        let reader = {
            let readers = self.readers.lock().unwrap();
            let set = readers.as_ref().ok_or_else(|| {
                ProviderError::InvalidData(format!("unknown Hamburg field '{external_id}'"))
            })?;
            set.fields.get(external_id).cloned().ok_or_else(|| {
                ProviderError::InvalidData(format!("unknown Hamburg field '{external_id}'"))
            })?
        };
        let mut reader = reader.lock().unwrap();
        let fetcher: &dyn ResourceFetcher = self.fetcher.as_ref();

        let mut measurements: Vec<SourceMeasurement> = Vec::with_capacity(budget);
        while measurements.len() < budget {
            // Both streams need a head to compare (a no-op when already set).
            reader.current.ensure_pending(fetcher)?;
            if let Some(legacy) = reader.legacy.as_mut() {
                legacy.ensure_pending(fetcher)?;
            }

            let current_head = reader.current.pending.clone();
            let legacy_head = match reader.legacy.as_ref() {
                Some(legacy) => legacy.pending.clone(),
                None => None,
            };

            let take_legacy = match (&legacy_head, &current_head) {
                (None, None) => break,
                (Some(_), None) => true,
                (None, Some(_)) => false,
                (Some(legacy), Some(current)) => {
                    if legacy.timestamp == current.timestamp
                        && legacy.resolution_seconds == current.resolution_seconds
                    {
                        // Overlap: the current (live) value wins; drop the legacy row.
                        reader.legacy.as_mut().expect("legacy present").pop();
                        false
                    } else {
                        (legacy.timestamp, legacy.resolution_seconds)
                            <= (current.timestamp, current.resolution_seconds)
                    }
                }
            };

            let row = if take_legacy {
                reader.legacy.as_mut().expect("legacy present").pop()
            } else {
                reader.current.pop()
            };
            let Some(row) = row else { break };
            // Exclusive lower bound: never re-emit the anchor boundary row.
            if anchor.is_none_or(|anchor| row.timestamp > anchor) {
                measurements.push(SourceMeasurement {
                    channel_external_id: external_id.to_string(),
                    record: row,
                });
            }
        }

        let last_real = measurements.last().map(|m| m.record.timestamp);
        let legacy_done = match reader.legacy.as_ref() {
            Some(legacy) => !legacy.has_more(),
            None => true,
        };
        let done = legacy_done && !reader.current.has_more();

        Ok(ChannelPage {
            measurements,
            last_real,
            next_from: last_real,
            done,
        })
    }
}

impl DataProvider for HamburgStaAdapter {
    fn check_health(&self) -> HealthStatus {
        let Some((host, port)) = parse_host_and_port(&self.base_url) else {
            return HealthStatus::Down("invalid base_url in provider config".to_string());
        };
        match TcpStream::connect((host, port)) {
            Ok(_) => HealthStatus::Up,
            Err(error) => HealthStatus::Down(format!("{error:?}")),
        }
    }

    fn get_all_counting_stations(&self) -> Result<Vec<CountingStationRecord>, ProviderError> {
        Ok(self.ensure_index()?.stations.clone())
    }

    fn get_all_channels(&self) -> Result<Vec<ChannelRecord>, ProviderError> {
        Ok(self.ensure_index()?.channels.clone())
    }

    fn get_measurements_source(
        &self,
        from: Option<DateTime<Utc>>,
        max_batch_size: usize,
    ) -> Result<SourceMeasurementBatch, ProviderError> {
        let index = self.ensure_index()?;
        let ids: Vec<String> = index
            .channels
            .iter()
            .map(|channel| channel.external_id.clone())
            .collect();

        self.ensure_readers(from, &ids, &index);

        // Pick up to `concurrency` not-yet-exhausted fields (fair round-robin),
        // de-duplicated so a short tail is never paged twice in one batch.
        let picks = {
            let mut guard = self.scanner.lock().unwrap();
            let needs_seed = match guard.as_ref() {
                Some(scanner) => !scanner.matches(from, &ids),
                None => true,
            };
            if needs_seed {
                *guard = Some(SourceScanner::new(from, &ids));
            }
            let scanner = guard.as_mut().expect("scanner seeded");
            let mut picks: Vec<(String, Option<DateTime<Utc>>)> = Vec::new();
            let mut seen: HashSet<String> = HashSet::new();
            let mut probes = 0;
            while picks.len() < self.concurrency && probes < ids.len() {
                probes += 1;
                match scanner.next_channel() {
                    Some((id, next)) => {
                        if seen.insert(id.clone()) {
                            picks.push((id, next));
                        }
                    }
                    None => break,
                }
            }
            picks
        };

        if picks.is_empty() {
            return Ok(SourceMeasurementBatch {
                measurements: vec![],
                next_from: None,
                more: false,
            });
        }

        let budget = max_batch_size.max(1);

        // Page every picked field concurrently. Per-field readers live behind
        // per-field locks, so the fetched channels never contend; a transient
        // failure of one field fails the batch (un-recorded fields are simply
        // read again on the next call).
        let results: Vec<(String, Result<ChannelPage, ProviderError>)> =
            std::thread::scope(|scope| {
                let handles: Vec<_> = picks
                    .into_iter()
                    .map(|(id, _)| {
                        scope.spawn(move || {
                            let page = self.fill_page(&id, budget, from);
                            (id, page)
                        })
                    })
                    .collect();
                handles
                    .into_iter()
                    .map(|handle| handle.join().expect("field pager panicked"))
                    .collect()
            });

        let mut guard = self.scanner.lock().unwrap();
        let scanner = guard.as_mut().expect("scanner seeded");
        let mut measurements: Vec<SourceMeasurement> = Vec::new();
        let mut next_from: Option<DateTime<Utc>> = None;
        let mut more = false;
        for (id, page) in results {
            let batch = scanner.record(&id, page?)?;
            measurements.extend(batch.measurements);
            next_from = batch.next_from;
            more = batch.more;
        }
        Ok(SourceMeasurementBatch {
            measurements,
            next_from,
            more,
        })
    }

    fn max_measurement_batch_size(&self) -> usize {
        self.max_measurement_batch_size
    }

    fn attach_provider_messages(&self, sink: Arc<dyn ProviderMessageSink + Send + Sync>) {
        *self.messages.lock().unwrap() = Some(sink);
    }
}

/// Percent-encodes an OData filter value for use as a URL query parameter.
fn filter_param(filter: &str) -> String {
    let mut out = String::new();
    for byte in filter.bytes() {
        match byte {
            b'A'..=b'Z' | b'a'..=b'z' | b'0'..=b'9' | b'-' | b'.' | b'_' | b'~' => {
                out.push(byte as char);
            }
            _ => out.push_str(&format!("%{byte:02X}")),
        }
    }
    out
}

/// Parses the host and port from a `https?://host[:port]/path` URL.
fn parse_host_and_port(url: &str) -> Option<(String, u16)> {
    let rest = url
        .strip_prefix("https://")
        .or_else(|| url.strip_prefix("http://"))?;
    let host_port = rest.split('/').next()?;
    let (host, port) = match host_port.split_once(':') {
        Some((host, port)) => (host, port.parse::<u16>().ok()?),
        None => (
            host_port,
            if url.starts_with("https://") { 443 } else { 80 },
        ),
    };
    Some((host.to_string(), port))
}

#[cfg(test)]
mod tests {
    use std::collections::HashMap;
    use std::sync::Arc;

    use chrono::{DateTime, Utc};

    use crate::adapter::driven::hamburg_sta::fetcher::ResourceFetcher;
    use crate::core::domain::configuration::configuration::value_objects::{
        DataProviderConfiguration, DataSourceConfiguration,
    };

    use super::{HamburgStaAdapter, filter_param, parse_host_and_port};

    fn config() -> DataSourceConfiguration {
        let provider = DataProviderConfiguration::new(
            HamburgStaAdapter::provider_type().to_string(),
            HashMap::new(),
        )
        .unwrap();
        DataSourceConfiguration::new("Hamburg".to_string(), provider).unwrap()
    }

    /// A fetcher that is never called; used only to build the adapter.
    struct StubFetcher;
    impl ResourceFetcher for StubFetcher {
        fn fetch(&self, url: &str) -> Result<String, String> {
            Err(format!("unexpected fetch: {url}"))
        }
    }

    #[test]
    fn encodes_filter_query_params() {
        assert_eq!(
            filter_param("properties/layerName eq 'Anzahl_Fahrraeder_Zaehlfeld_5-Min'"),
            "properties%2FlayerName%20eq%20%27Anzahl_Fahrraeder_Zaehlfeld_5-Min%27"
        );
    }

    #[test]
    fn parses_host_and_port() {
        assert_eq!(
            parse_host_and_port("https://iot.hamburg.de/v1.0/"),
            Some(("iot.hamburg.de".to_string(), 443))
        );
        assert_eq!(
            parse_host_and_port("http://localhost:8080/"),
            Some(("localhost".to_string(), 8080))
        );
    }

    #[test]
    fn observations_url_is_uri_parseable() {
        // Regression: the `$orderby` value must be percent-encoded. ureq 3 parses
        // URLs with `http::Uri`, which rejects a literal space in the query
        // string ("http: invalid uri character"), unlike ureq 2.
        let adapter = HamburgStaAdapter::with_fetcher(&config(), Arc::new(StubFetcher)).unwrap();
        let from: DateTime<Utc> = "2026-01-02T00:00:00Z".parse().unwrap();
        let to: DateTime<Utc> = "2026-01-03T00:00:00Z".parse().unwrap();

        for url in [
            adapter.observations_url(30072, None, None),
            adapter.observations_url(30072, Some(from), Some(to)),
        ] {
            let uri: ureq::http::Uri = url.parse().unwrap_or_else(|e| {
                panic!("observations URL is not a valid http::Uri: {url} -> {e}")
            });
            assert_eq!(uri.scheme_str(), Some("https"));
            assert!(
                !url.contains("phenomenonTime asc"),
                "literal space in URL: {url}"
            );
        }
    }
}
