//! The Bonn Open Data [`DataProvider`] adapter: configuration, the in-memory
//! cache, and measurement serving. HTTP and parsing live in the sibling modules
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
    BonnIndex, build_index, drop_aggregate_stations, parse_host_and_port, parse_measurements_csv,
    parse_stations_geojson, parse_yearly_hourly_csv, station_name_map,
};

pub(crate) const PROVIDER_TYPE: &str = "bonn_opendata_http_provider";
pub(crate) const DEFAULT_MAX_MEASUREMENT_BATCH_SIZE: usize = 500;
pub(crate) const DEFAULT_CACHE_DURATION_SECS: u64 = 300;

pub struct BonnOpendataAdapter {
    stations_url: String,
    measurements_url: String,
    /// Optional per-year **wide** hourly CSVs (2023–2025) for the backfill.
    historical_urls: Vec<String>,
    max_measurement_batch_size: usize,
    cache_duration: Duration,
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
    index: Arc<BonnIndex>,
}

impl BonnOpendataAdapter {
    pub fn provider_type() -> &'static str {
        PROVIDER_TYPE
    }

    /// Builds the adapter from the data source's provider vars.
    ///
    /// Required vars: `stations_url`, `measurements_url`. Optional vars:
    /// `historical_urls` (space-separated yearly wide hourly CSVs),
    /// `max_measurement_batch_size` (default `500`), `cache_duration` (seconds,
    /// default `300`). A missing/invalid value is a configuration error (blocks
    /// startup).
    pub fn new(config: &DataSourceConfiguration) -> Result<Self, ConfigError> {
        Self::with_fetcher(config, Arc::new(HttpResourceFetcher))
    }

    pub(crate) fn with_fetcher(
        config: &DataSourceConfiguration,
        fetcher: Arc<dyn ResourceFetcher>,
    ) -> Result<Self, ConfigError> {
        let stations_url = config
            .provider()
            .var("stations_url")
            .ok_or_else(|| {
                ConfigError::InvalidFormat(format!(
                    "{PROVIDER_TYPE}: missing required var 'stations_url'"
                ))
            })?
            .to_string();

        let measurements_url = config
            .provider()
            .var("measurements_url")
            .ok_or_else(|| {
                ConfigError::InvalidFormat(format!(
                    "{PROVIDER_TYPE}: missing required var 'measurements_url'"
                ))
            })?
            .to_string();

        let historical_urls = config
            .provider()
            .var("historical_urls")
            .map(|raw| raw.split_whitespace().map(str::to_string).collect())
            .unwrap_or_default();

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

        Ok(Self {
            stations_url,
            measurements_url,
            historical_urls,
            max_measurement_batch_size,
            cache_duration: Duration::from_secs(cache_duration),
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

    /// The configured historical resource URLs (test-only accessor).
    #[cfg(test)]
    pub(crate) fn historical_urls(&self) -> &[String] {
        &self.historical_urls
    }

    /// The cached index, if any (test-only accessor).
    #[cfg(test)]
    pub(crate) fn cache_index(&self) -> Option<Arc<BonnIndex>> {
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

    /// Ensures a usable parsed index exists and returns it.
    ///
    /// The cache-freshness check runs **inline** under a single lock acquisition
    /// (the guard is never held while re-locking `self.cache`), so the
    /// non-reentrant `Mutex` is never acquired twice on the same thread.
    fn ensure_index(&self) -> Result<Arc<BonnIndex>, ProviderError> {
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
        let stations = drop_aggregate_stations(stations);
        let station_by_name = station_name_map(&stations);

        // Historical wide CSVs: best-effort. A missing/rotated yearly file skips
        // that year with a WARNING instead of failing the whole import.
        let mut yearly_rows = Vec::new();
        for url in &self.historical_urls {
            match self.fetcher.fetch(url) {
                Ok(csv) => {
                    let parsed =
                        parse_yearly_hourly_csv(&csv, &station_by_name, messages.as_deref())?;
                    yearly_rows.extend(parsed);
                }
                Err(message) => {
                    self.emit(
                        ProviderMessageSeverity::Warning,
                        format!(
                            "historical measurements fetch failed ({url}): {message}; year skipped"
                        ),
                    );
                }
            }
        }

        // Current Vortag CSV: a failure here is fatal.
        let measurements_csv = self
            .fetcher
            .fetch(&self.measurements_url)
            .map_err(|message| {
                ProviderError::Unreachable(format!("measurements fetch failed: {message}"))
            })?;
        let vortag_rows = parse_measurements_csv(&measurements_csv, messages.as_deref())?;

        let index = Arc::new(build_index(
            stations,
            vortag_rows,
            yearly_rows,
            messages.as_deref(),
        ));

        let row_count: usize = index.rows.values().map(Vec::len).sum();
        self.emit(
            ProviderMessageSeverity::Info,
            format!(
                "bonn data refreshed: {} stations, {} channels, {} rows",
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
    fn page_channel(
        &self,
        external_id: &str,
        from: Option<DateTime<Utc>>,
        budget: usize,
    ) -> Result<ChannelPage, ProviderError> {
        let index = self.ensure_index()?;
        let records = index.rows.get(external_id).cloned().unwrap_or_default();

        let mut filtered: Vec<MeasurementRecord> = records
            .into_iter()
            .filter(|record| from.is_none_or(|f| record.timestamp > f))
            .collect();

        // Rows are already sorted ascending (see `build_index`).
        let batch_size_limit_reached = filtered.len() > budget;
        filtered.truncate(budget);
        let last_real = filtered.last().map(|record| record.timestamp);
        let measurements = filtered
            .into_iter()
            .map(|record| SourceMeasurement {
                channel_external_id: external_id.to_string(),
                record,
            })
            .collect();

        Ok(ChannelPage {
            measurements,
            last_real,
            next_from: last_real,
            done: !batch_size_limit_reached,
        })
    }
}

impl DataProvider for BonnOpendataAdapter {
    fn check_health(&self) -> HealthStatus {
        let (host, port) = match parse_host_and_port(&self.measurements_url) {
            Some(host_port) => host_port,
            None => {
                return HealthStatus::Down(
                    "invalid measurements_url in provider config".to_string(),
                );
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
