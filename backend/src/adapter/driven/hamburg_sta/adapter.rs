//! The Hamburg SensorThings [`DataProvider`] adapter: configuration, discovery
//! and measurement serving.
//!
//! Discovery is cheap: one paginated `Datastreams` query filtered by
//! `properties/layerName eq 'Anzahl_Fahrraeder_Zaehlfeld_5-Min'` (with
//! `$expand=Thing`), which yields the live `Zählfeld` datastreams *and* the
//! legacy `(veraltet)` ones that extend each field's history. The parsed index
//! is cached for `cache_duration` seconds. Observations are fetched live,
//! paged via `@iot.nextLink`, and the legacy + current rows are merged per field
//! (dedup keep-last, current wins).

use std::net::TcpStream;
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use chrono::{DateTime, Utc};

use crate::core::domain::configuration::configuration::value_objects::DataSourceConfiguration;
use crate::core::domain::configuration::error::ConfigError;
use crate::core::domain::data_source::provider_message::ProviderMessageSeverity;
use crate::core::domain::data_source::provider_port::{
    ChannelRecord, CountingStationRecord, DataProvider, MeasurementBatch, MeasurementQuery,
    MeasurementRecord, ProviderError, ProviderMessageSink,
};
use crate::core::domain::health::HealthStatus;

use super::fetcher::{HttpResourceFetcher, ResourceFetcher};
use super::parsing::{
    HamburgIndex, LAYER_NAME, RawDatastream, RawObservation, RawPage, build_index,
    observations_url, parse_observation,
};

pub(crate) const PROVIDER_TYPE: &str = "hamburg_sta_http_provider";
pub(crate) const DEFAULT_BASE_URL: &str = "https://iot.hamburg.de/v1.0/";
pub(crate) const DEFAULT_MAX_MEASUREMENT_BATCH_SIZE: usize = 500;
pub(crate) const DEFAULT_CACHE_DURATION_SECS: u64 = 300;
const OBSERVATIONS_PAGE_SIZE: usize = 1000;
const DISCOVERY_PAGE_SIZE: usize = 500;

pub struct HamburgStaAdapter {
    base_url: String,
    max_measurement_batch_size: usize,
    cache_duration: Duration,
    include_legacy: bool,
    fetcher: Arc<dyn ResourceFetcher>,
    /// Scoped provider-message sink, attached by `StartupService`. `None` until
    /// attached.
    messages: Mutex<Option<Arc<dyn ProviderMessageSink + Send + Sync>>>,
    /// In-memory cache of the parsed index.
    cache: Mutex<Option<CachedData>>,
    /// Serializes cache refresh across threads.
    refresh_lock: Mutex<()>,
}

#[derive(Clone)]
struct CachedData {
    fetched_at: Instant,
    index: Arc<HamburgIndex>,
}

impl HamburgStaAdapter {
    pub fn provider_type() -> &'static str {
        PROVIDER_TYPE
    }

    /// Builds the adapter from the data source's provider vars.
    ///
    /// Optional vars: `base_url` (default the official Hamburg SensorThings
    /// root), `max_measurement_batch_size` (default `500`), `cache_duration`
    /// (seconds, default `300`), `include_legacy` (default `true`, merges the
    /// `(veraltet)` field series for history). A missing/invalid value is a
    /// configuration error (blocks startup).
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

        Ok(Self {
            base_url,
            max_measurement_batch_size,
            cache_duration: Duration::from_secs(cache_duration),
            include_legacy,
            fetcher,
            messages: Mutex::new(None),
            cache: Mutex::new(None),
            refresh_lock: Mutex::new(()),
        })
    }

    /// The configured cache window in seconds.
    pub fn cache_duration_secs(&self) -> u64 {
        self.cache_duration.as_secs()
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

    /// Fetches observations of one datastream, following `@iot.nextLink` pages,
    /// up to `budget` parsed rows. Returns whether more rows remain.
    fn fetch_field_observations(
        &self,
        datastream_id: i64,
        from: Option<DateTime<Utc>>,
        to: Option<DateTime<Utc>>,
        budget: usize,
    ) -> Result<(Vec<MeasurementRecord>, bool), ProviderError> {
        let mut url = self.observations_url(datastream_id, from, to);
        let mut rows = Vec::new();
        let mut truncated = false;
        loop {
            let json = self.fetcher.fetch(&url).map_err(|message| {
                ProviderError::Unreachable(format!(
                    "observations fetch failed (datastream {datastream_id}): {message}"
                ))
            })?;
            let page: RawPage<RawObservation> = serde_json::from_str(&json).map_err(|e| {
                ProviderError::InvalidData(format!("invalid Observations json: {e}"))
            })?;
            for observation in page.value {
                if let Some(record) = parse_observation(&observation) {
                    rows.push(record);
                }
            }
            match page.next_link {
                Some(next) if rows.len() < budget => url = next,
                Some(_) => {
                    truncated = true;
                    break;
                }
                None => break,
            }
        }
        Ok((rows, truncated))
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
        if filter.is_empty() {
            format!("{base}?$orderby=phenomenonTime asc&$top={OBSERVATIONS_PAGE_SIZE}")
        } else {
            format!(
                "{base}?$filter={}&$orderby=phenomenonTime asc&$top={OBSERVATIONS_PAGE_SIZE}",
                filter_param(&filter),
            )
        }
    }

    /// Merges legacy + current rows for one field: dedup keep-last on
    /// `(timestamp, resolution_seconds)` so the current (live) value wins on an
    /// overlap, sorted ascending.
    fn merge_rows(mut rows: Vec<MeasurementRecord>) -> Vec<MeasurementRecord> {
        rows.sort_by_key(|r| (r.timestamp, r.resolution_seconds));
        let mut merged: Vec<MeasurementRecord> = Vec::with_capacity(rows.len());
        for row in rows {
            if let Some(last) = merged.last_mut()
                && last.timestamp == row.timestamp
                && last.resolution_seconds == row.resolution_seconds
            {
                *last = row;
            } else {
                merged.push(row);
            }
        }
        merged
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

    fn get_measurements(&self, query: MeasurementQuery) -> Result<MeasurementBatch, ProviderError> {
        let index = self.ensure_index()?;
        let field_id = query
            .channel
            .external_datasource_id
            .as_ref()
            .map(|external| external.0.clone())
            .ok_or_else(|| ProviderError::InvalidData("channel has no external id".to_string()))?;
        let sources = index.fields.get(&field_id).ok_or_else(|| {
            ProviderError::InvalidData(format!("unknown Hamburg field '{field_id}'"))
        })?;

        // Fetch legacy first, then current, so the stable dedup keep-last makes
        // the current (live) value win on overlapping rows.
        let mut rows: Vec<MeasurementRecord> = Vec::new();
        let mut truncated = false;
        if self.include_legacy
            && let Some(legacy_id) = sources.legacy
        {
            let (mut legacy, legacy_truncated) = self.fetch_field_observations(
                legacy_id,
                query.from,
                query.to,
                query.max_batch_size,
            )?;
            truncated |= legacy_truncated;
            rows.append(&mut legacy);
        }
        if let Some(current_id) = sources.current {
            let (mut current, current_truncated) = self.fetch_field_observations(
                current_id,
                query.from,
                query.to,
                query.max_batch_size,
            )?;
            truncated |= current_truncated;
            rows.append(&mut current);
        }

        let filtered: Vec<_> = Self::merge_rows(rows)
            .into_iter()
            .filter(|record| {
                query.from.is_none_or(|from| record.timestamp > from)
                    && query.to.is_none_or(|to| record.timestamp <= to)
            })
            .collect();

        let mut filtered = filtered;
        let batch_size_limit_reached = truncated || filtered.len() > query.max_batch_size;
        filtered.truncate(query.max_batch_size);
        let last_measurement_datetime = filtered.last().map(|record| record.timestamp);

        Ok(MeasurementBatch {
            measurements: filtered,
            last_measurement_datetime,
            batch_size_limit_reached,
            // The SensorThings API serves the whole requested window; paging is
            // row-count based only.
            timeframe_limit_reached: false,
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
    use super::{filter_param, parse_host_and_port};

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
}
