//! The Leipzig WFS [`DataProvider`] adapter: configuration, the in-memory cache
//! and measurement serving. HTTP and parsing live in the sibling modules
//! [`fetcher`](super::fetcher) and [`parsing`](super::parsing).

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
    LeipzigIndex, PageInfo, build_index, paged_url, parse_daily_page, parse_host_and_port,
    parse_hourly_page, parse_stations_geojson,
};

pub(crate) const PROVIDER_TYPE: &str = "leipzig_wfs_http_provider";
pub(crate) const DEFAULT_MAX_MEASUREMENT_BATCH_SIZE: usize = 500;
pub(crate) const DEFAULT_CACHE_DURATION_SECS: u64 = 300;
pub(crate) const DEFAULT_WFS_PAGE_SIZE: usize = 5000;

pub struct LeipzigWfsAdapter {
    stations_url: String,
    hourly_url: String,
    daily_url: String,
    max_measurement_batch_size: usize,
    cache_duration: Duration,
    /// `count` per WFS page when fetching the time-series layers.
    wfs_page_size: usize,
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
}

#[derive(Clone)]
struct CachedData {
    fetched_at: Instant,
    index: Arc<LeipzigIndex>,
}

/// Parses one WFS page body into typed rows plus its pagination info.
type PageParser<T> = fn(
    &str,
    Option<&(dyn ProviderMessageSink + Send + Sync)>,
) -> Result<(Vec<T>, PageInfo), ProviderError>;

impl LeipzigWfsAdapter {
    pub fn provider_type() -> &'static str {
        PROVIDER_TYPE
    }

    /// Builds the adapter from the data source's provider vars.
    ///
    /// Required vars: `stations_url`, `hourly_url`, `daily_url`. Optional vars:
    /// `max_measurement_batch_size` (default `500`), `cache_duration` (seconds,
    /// default `300`), `wfs_page_size` (`count` per WFS page, default `5000`). A
    /// missing/invalid value is a configuration error (blocks startup).
    pub fn new(config: &DataSourceConfiguration) -> Result<Self, ConfigError> {
        Self::with_fetcher(config, Arc::new(HttpResourceFetcher))
    }

    pub(crate) fn with_fetcher(
        config: &DataSourceConfiguration,
        fetcher: Arc<dyn ResourceFetcher>,
    ) -> Result<Self, ConfigError> {
        let required_var = |name: &str| {
            config
                .provider()
                .var(name)
                .map(str::to_string)
                .ok_or_else(|| {
                    ConfigError::InvalidFormat(format!(
                        "{PROVIDER_TYPE}: missing required var '{name}'"
                    ))
                })
        };
        let stations_url = required_var("stations_url")?;
        let hourly_url = required_var("hourly_url")?;
        let daily_url = required_var("daily_url")?;

        let parse_usize = |name: &str, default: usize| -> Result<usize, ConfigError> {
            match config.provider().var(name) {
                Some(raw) => raw.parse::<usize>().map_err(|_| {
                    ConfigError::InvalidFormat(format!(
                        "{PROVIDER_TYPE}: var '{name}' is not a valid number"
                    ))
                }),
                None => Ok(default),
            }
        };
        let max_measurement_batch_size = parse_usize(
            "max_measurement_batch_size",
            DEFAULT_MAX_MEASUREMENT_BATCH_SIZE,
        )?;
        let wfs_page_size = parse_usize("wfs_page_size", DEFAULT_WFS_PAGE_SIZE)?.max(1);
        let cache_duration = parse_usize("cache_duration", DEFAULT_CACHE_DURATION_SECS as usize)?;

        Ok(Self {
            stations_url,
            hourly_url,
            daily_url,
            max_measurement_batch_size,
            cache_duration: Duration::from_secs(cache_duration as u64),
            wfs_page_size,
            fetcher,
            messages: Mutex::new(None),
            cache: Mutex::new(None),
            refresh_lock: Mutex::new(()),
            scanner: Mutex::new(None),
        })
    }

    /// The configured cache window in seconds.
    pub fn cache_duration_secs(&self) -> u64 {
        self.cache_duration.as_secs()
    }

    /// The WFS page size (test-only accessor).
    #[cfg(test)]
    pub(crate) fn wfs_page_size(&self) -> usize {
        self.wfs_page_size
    }

    /// The cached index, if any (test-only accessor).
    #[cfg(test)]
    pub(crate) fn cache_index(&self) -> Option<Arc<LeipzigIndex>> {
        self.cache
            .lock()
            .unwrap()
            .as_ref()
            .map(|cached| cached.index.clone())
    }

    /// The attached provider-message sink, if any (cloned handle for callers).
    fn messages_sink(&self) -> Option<Arc<dyn ProviderMessageSink + Send + Sync>> {
        self.messages.lock().unwrap().clone()
    }

    /// Emits a scoped provider message (best-effort). Recording a message is
    /// never fatal for the provider's data-serving work, so a store failure here
    /// is swallowed rather than propagated.
    pub(crate) fn emit(&self, severity: ProviderMessageSeverity, message: impl AsRef<str>) {
        if let Some(sink) = self.messages.lock().unwrap().as_ref() {
            let _ = sink.provider_event_occurred(severity, message.as_ref());
        }
    }

    /// Fetches and parses every page of one time-series layer.
    ///
    /// `parse` maps one page body to its rows plus pagination info; `kind` names
    /// the layer for diagnostics. Paging stops when
    /// [`PageInfo::next_start_index`](super::parsing::PageInfo::next_start_index)
    /// reports the source as exhausted.
    fn fetch_time_series_pages<T>(
        &self,
        url: &str,
        kind: &str,
        parse: PageParser<T>,
    ) -> Result<Vec<T>, ProviderError> {
        let messages = self.messages_sink();
        let mut rows = Vec::new();
        let mut start_index = 0usize;
        loop {
            let page_url = paged_url(url, self.wfs_page_size, start_index);
            let json = self.fetcher.fetch(&page_url).map_err(|message| {
                ProviderError::Unreachable(format!("{kind} fetch failed: {message}"))
            })?;
            let (page_rows, info) = parse(&json, messages.as_deref())?;
            rows.extend(page_rows);
            match info.next_start_index(start_index, self.wfs_page_size) {
                Some(next) => start_index = next,
                None => break,
            }
        }
        Ok(rows)
    }

    /// Ensures a usable parsed index exists and returns it.
    ///
    /// The cache-freshness check runs **inline** under a single lock acquisition
    /// (the guard is never held while re-locking `self.cache`), so the
    /// non-reentrant `Mutex` is never acquired twice on the same thread.
    fn ensure_index(&self) -> Result<Arc<LeipzigIndex>, ProviderError> {
        // Fast path: cached and fresh. The guard is never re-locked: the
        // condition only reads through the borrowed `cached`.
        if let Some(cached) = self.cache.lock().unwrap().as_ref()
            && cached.fetched_at.elapsed() < self.cache_duration
        {
            return Ok(cached.index.clone());
        }

        // Slow path: serialize the refresh, then re-check under the lock.
        let _guard = self
            .refresh_lock
            .lock()
            .map_err(|_| ProviderError::Storage("refresh lock poisoned".to_string()))?;
        if let Some(cached) = self.cache.lock().unwrap().as_ref()
            && cached.fetched_at.elapsed() < self.cache_duration
        {
            return Ok(cached.index.clone());
        }

        let messages = self.messages_sink();

        // Stations: a failure here is fatal (the source is unavailable).
        let stations_json = self.fetcher.fetch(&self.stations_url).map_err(|message| {
            ProviderError::Unreachable(format!("stations fetch failed: {message}"))
        })?;
        let stations = parse_stations_geojson(&stations_json, messages.as_deref())?;

        let hourly_rows = self.fetch_time_series_pages(
            &self.hourly_url,
            "hourly measurements",
            parse_hourly_page,
        )?;
        let daily_rows =
            self.fetch_time_series_pages(&self.daily_url, "daily measurements", parse_daily_page)?;

        let index = Arc::new(build_index(
            stations,
            hourly_rows,
            daily_rows,
            messages.as_deref(),
        ));

        let row_count: usize = index.rows.values().map(Vec::len).sum();
        self.emit(
            ProviderMessageSeverity::Info,
            format!(
                "leipzig data refreshed: {} stations, {} channels, {} rows",
                index.stations.len(),
                index.channels.len(),
                row_count,
            ),
        );

        *self.cache.lock().unwrap() = Some(CachedData {
            fetched_at: Instant::now(),
            index: index.clone(),
        });
        Ok(index)
    }

    /// Serves the next page of measurements of one channel starting strictly
    /// after `from`, tagged with the channel's external id. The whole index is
    /// in memory and pre-sorted, so paging is row-count based only.
    ///
    /// Truncation stops at a **timestamp boundary**: a channel may hold two rows
    /// with the same timestamp (a daily row and the hourly row at the same local
    /// midnight) at different resolutions, and a single shared `imported_until`
    /// watermark can only advance past a timestamp once every row at it has been
    /// emitted. Splitting such a group across pages would drop the tail rows.
    fn page_channel(
        &self,
        external_id: &str,
        from: Option<DateTime<Utc>>,
        budget: usize,
    ) -> Result<ChannelPage, ProviderError> {
        let index = self.ensure_index()?;
        let records = index.rows.get(external_id).cloned().unwrap_or_default();

        let filtered: Vec<MeasurementRecord> = records
            .into_iter()
            .filter(|record| from.is_none_or(|f| record.timestamp > f))
            .collect();

        // Rows are already sorted ascending (see `build_index`).
        let budget = budget.max(1);
        let limit = budget.min(filtered.len());
        let mut end = limit;
        // Never split a group of equal timestamps across pages.
        while end < filtered.len() && filtered[end].timestamp == filtered[end - 1].timestamp {
            end += 1;
        }
        let batch_size_limit_reached = end < filtered.len();
        let measurements: Vec<SourceMeasurement> = filtered[..end]
            .iter()
            .map(|record| SourceMeasurement {
                channel_external_id: external_id.to_string(),
                record: record.clone(),
            })
            .collect();
        let last_real = measurements
            .last()
            .map(|measurement| measurement.record.timestamp);

        Ok(ChannelPage {
            measurements,
            last_real,
            next_from: last_real,
            done: !batch_size_limit_reached,
        })
    }
}

impl DataProvider for LeipzigWfsAdapter {
    fn check_health(&self) -> HealthStatus {
        let (host, port) = match parse_host_and_port(&self.stations_url) {
            Some(host_port) => host_port,
            None => {
                return HealthStatus::Down("invalid stations_url in provider config".to_string());
            }
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

        let pick = {
            let mut guard = self.scanner.lock().unwrap();
            let needs_seed = match guard.as_ref() {
                Some(scanner) => !scanner.matches(from, &ids),
                None => true,
            };
            if needs_seed {
                *guard = Some(SourceScanner::new(from, &ids));
            }
            guard.as_mut().expect("scanner seeded").next_channel()
        };

        let Some((id, next)) = pick else {
            return Ok(SourceMeasurementBatch {
                measurements: vec![],
                next_from: None,
                more: false,
            });
        };

        let page = self.page_channel(&id, next, max_batch_size)?;
        let mut guard = self.scanner.lock().unwrap();
        guard.as_mut().expect("scanner seeded").record(&id, page)
    }

    fn max_measurement_batch_size(&self) -> usize {
        self.max_measurement_batch_size
    }

    fn attach_provider_messages(&self, sink: Arc<dyn ProviderMessageSink + Send + Sync>) {
        *self.messages.lock().unwrap() = Some(sink);
    }
}
