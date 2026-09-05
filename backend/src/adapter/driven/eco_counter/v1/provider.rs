//! The API_V1 mode [`DataProvider`]: imports the counters listed in the bundled
//! YAML catalog ([`stations.yml`](stations.yml)) from the legacy public Eco-Visio
//! API via the [`PublicWebpageClient`](super::client).
//!
//! The upstream no longer auto-discovers German stations (they moved to the
//! API-key-gated platform), so the stations are listed in the catalog instead of
//! the runtime TOML. The live metadata (token/domain/coordinates) is fetched per
//! counter and cached for `cache_duration`; measurements are paged from
//! `publicwebpage/data/{idPdc}` over day windows.

use std::collections::HashMap;
use std::net::TcpStream;
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use chrono::{DateTime, Days, Utc};

use crate::adapter::driven::eco_counter::common::{parse_host_and_port, utc_midnight};
use crate::adapter::driven::eco_counter::fetcher::{HttpResourceFetcher, ResourceFetcher};
use crate::adapter::driven::source_merge::{ChannelPage, SourceScanner};
use crate::core::domain::configuration::configuration::value_objects::DataSourceConfiguration;
use crate::core::domain::configuration::error::ConfigError;
use crate::core::domain::data_source::provider_message::ProviderMessageSeverity;
use crate::core::domain::data_source::provider_port::{
    ChannelRecord, CountingStationRecord, DataProvider, ProviderError, ProviderMessageSink,
    SourceMeasurement, SourceMeasurementBatch,
};
use crate::core::domain::health::HealthStatus;

use super::catalog::{CatalogStation, DEFAULT_STATIONS_YAML, parse_catalog};
use super::client::{DEFAULT_BASE_URL, PublicWebpageClient};
use super::parsing::{EcoIndex, RawSiteMetadata, build_index, parse_row, resolution_for_step};

/// This mode's key in the `modes` list.
pub const MODE: &str = "api_v1";
/// Provider-var prefix for this mode (`v1_…`): each mode reads its own vars
/// (`v1_base_url`, `v1_step`, …) so several modes can share one data source.
pub(crate) const VAR_PREFIX: &str = "v1_";
pub(crate) const DEFAULT_MAX_MEASUREMENT_BATCH_SIZE: usize = 500;
pub(crate) const DEFAULT_CACHE_DURATION_SECS: u64 = 300;
/// Default `step` (data resolution) when not configured: `3` = hourly.
pub(crate) const DEFAULT_STEP: i64 = 3;
/// Default day window requested per HTTP call.
pub(crate) const DEFAULT_PAGE_DAYS: i64 = 7;
/// Default initial lookback (days) when no `imported_until` watermark exists.
pub(crate) const DEFAULT_IMPORT_DAYS_BACK: i64 = 365;

/// Reads a provider var scoped to this mode (`v1_<name>`), so several modes
/// configured in one data source each read their own value.
fn mode_var<'a>(config: &'a DataSourceConfiguration, name: &str) -> Option<&'a str> {
    config.provider().var(&format!("{VAR_PREFIX}{name}"))
}

pub struct EcoCounterV1Provider {
    base_url: String,
    step: i64,
    resolution_seconds: i64,
    max_measurement_batch_size: usize,
    cache_duration: Duration,
    page_days: i64,
    import_days_back: i64,
    catalog: Vec<CatalogStation>,
    client: PublicWebpageClient,
    /// Scoped provider-message sink, attached by `StartupService`.
    messages: Mutex<Option<Arc<dyn ProviderMessageSink + Send + Sync>>>,
    /// In-memory cache of the parsed index (stations/channels/site access).
    index: Mutex<Option<CachedIndex>>,
    /// Serializes index refresh across threads.
    refresh_lock: Mutex<()>,
    /// Whole-source (channel-interleaved) reader state for the current run.
    scanner: Mutex<Option<ScannerState>>,
}

struct CachedIndex {
    fetched_at: Instant,
    index: Arc<EcoIndex>,
}

/// Per-run measurement reader state: the run anchor plus a fair round-robin
/// scanner over the channel ids.
struct ScannerState {
    anchor: Option<DateTime<Utc>>,
    ids: Vec<String>,
    scanner: SourceScanner,
}

impl EcoCounterV1Provider {
    pub fn mode() -> &'static str {
        MODE
    }

    /// Builds the provider from the data source's provider vars.
    ///
    /// All vars are read with the `v1_` mode prefix. Optional vars:
    /// `v1_base_url`, `v1_stations` (path to a YAML catalog; defaults to the
    /// bundled [`stations.yml`](stations.yml)), `v1_step` (`2` = 15 min, `3` =
    /// hourly, `4` = daily; default `3`), `v1_max_measurement_batch_size`
    /// (default `500`), `v1_cache_duration` (seconds, default `300`),
    /// `v1_page_days` (default `7`), `v1_import_days_back` (default `365`).
    /// Missing/invalid values are configuration errors (block startup).
    pub fn new(config: &DataSourceConfiguration) -> Result<Self, ConfigError> {
        let catalog_source = match mode_var(config, "stations") {
            Some(path) => std::fs::read_to_string(path).map_err(|error| {
                ConfigError::InvalidFormat(format!(
                    "{MODE}: cannot read stations catalog '{path}': {error}"
                ))
            })?,
            None => DEFAULT_STATIONS_YAML.to_string(),
        };
        let catalog = parse_catalog(&catalog_source)
            .map_err(|message| ConfigError::InvalidFormat(format!("{MODE}: {message}")))?;
        Self::with_fetcher(config, catalog, Arc::new(HttpResourceFetcher::new()))
    }

    pub(crate) fn with_fetcher(
        config: &DataSourceConfiguration,
        catalog: Vec<CatalogStation>,
        fetcher: Arc<dyn ResourceFetcher>,
    ) -> Result<Self, ConfigError> {
        let base_url = mode_var(config, "base_url")
            .unwrap_or(DEFAULT_BASE_URL)
            .trim_end_matches('/')
            .to_string();
        if base_url.is_empty() {
            return Err(ConfigError::InvalidFormat(format!(
                "{MODE}: var 'v1_base_url' must not be empty"
            )));
        }

        let step = match mode_var(config, "step") {
            Some(raw) => raw.parse::<i64>().map_err(|_| {
                ConfigError::InvalidFormat(format!("{MODE}: var 'v1_step' is not a valid number"))
            })?,
            None => DEFAULT_STEP,
        };
        let resolution_seconds = resolution_for_step(step).ok_or_else(|| {
            ConfigError::InvalidFormat(format!(
                "{MODE}: var 'v1_step' must be 2 (15 min), 3 (hourly) or 4 (daily)"
            ))
        })?;

        let max_measurement_batch_size = match mode_var(config, "max_measurement_batch_size") {
            Some(raw) => raw.parse::<usize>().map_err(|_| {
                ConfigError::InvalidFormat(format!(
                    "{MODE}: var 'v1_max_measurement_batch_size' is not a valid number"
                ))
            })?,
            None => DEFAULT_MAX_MEASUREMENT_BATCH_SIZE,
        };

        let cache_duration = match mode_var(config, "cache_duration") {
            Some(raw) => raw.parse::<u64>().map_err(|_| {
                ConfigError::InvalidFormat(format!(
                    "{MODE}: var 'v1_cache_duration' is not a valid number"
                ))
            })?,
            None => DEFAULT_CACHE_DURATION_SECS,
        };

        let page_days = match mode_var(config, "page_days") {
            Some(raw) => raw.parse::<i64>().map_err(|_| {
                ConfigError::InvalidFormat(format!(
                    "{MODE}: var 'v1_page_days' is not a valid number"
                ))
            })?,
            None => DEFAULT_PAGE_DAYS,
        }
        .max(1);

        let import_days_back = match mode_var(config, "import_days_back") {
            Some(raw) => raw.parse::<i64>().map_err(|_| {
                ConfigError::InvalidFormat(format!(
                    "{MODE}: var 'v1_import_days_back' is not a valid number"
                ))
            })?,
            None => DEFAULT_IMPORT_DAYS_BACK,
        }
        .max(1);

        if catalog.is_empty() {
            return Err(ConfigError::InvalidFormat(
                "{MODE}: the stations catalog is empty; add counters to v1/stations.yml"
                    .to_string(),
            ));
        }

        Ok(Self {
            base_url: base_url.clone(),
            step,
            resolution_seconds,
            max_measurement_batch_size,
            cache_duration: Duration::from_secs(cache_duration),
            page_days,
            import_days_back,
            catalog,
            client: PublicWebpageClient::new(base_url, fetcher),
            messages: Mutex::new(None),
            index: Mutex::new(None),
            refresh_lock: Mutex::new(()),
            scanner: Mutex::new(None),
        })
    }

    /// The configured data resolution step.
    pub fn step(&self) -> i64 {
        self.step
    }

    /// The configured discovery cache window in seconds.
    pub fn cache_duration_secs(&self) -> u64 {
        self.cache_duration.as_secs()
    }

    fn messages_sink(&self) -> Option<Arc<dyn ProviderMessageSink + Send + Sync>> {
        self.messages.lock().unwrap().clone()
    }

    /// Emits a scoped provider message (best-effort).
    pub(crate) fn emit(&self, severity: ProviderMessageSeverity, message: impl AsRef<str>) {
        if let Some(sink) = self.messages.lock().unwrap().as_ref() {
            let _ = sink.provider_event_occurred(severity, message.as_ref());
        }
    }

    /// Ensures a usable parsed index exists and returns it (refreshes the live
    /// per-station metadata when the cache expired).
    fn ensure_index(&self) -> Result<Arc<EcoIndex>, ProviderError> {
        if let Some(cached) = self.index.lock().unwrap().as_ref()
            && cached.fetched_at.elapsed() < self.cache_duration
        {
            return Ok(cached.index.clone());
        }
        let _guard = self
            .refresh_lock
            .lock()
            .map_err(|_| ProviderError::Storage("refresh lock poisoned".to_string()))?;
        if let Some(cached) = self.index.lock().unwrap().as_ref()
            && cached.fetched_at.elapsed() < self.cache_duration
        {
            return Ok(cached.index.clone());
        }

        let messages = self.messages_sink();
        let mut metadata: HashMap<i64, RawSiteMetadata> =
            HashMap::with_capacity(self.catalog.len());
        for station in &self.catalog {
            metadata.insert(station.id, self.client.metadata(station.id)?);
        }
        let index = Arc::new(build_index(&self.catalog, &metadata, messages.as_deref()));

        // A non-empty catalog that resolves to zero usable stations means every
        // counter migrated or is misconfigured — surface it instead of silently
        // "importing" nothing.
        if index.sites.is_empty() {
            return Err(ProviderError::Unreachable(
                "eco-counter v1: no catalog station resolved to a usable counter (all migrated \
                 or not public?)"
                    .to_string(),
            ));
        }

        self.emit(
            ProviderMessageSeverity::Info,
            format!(
                "eco-counter v1 discovery refreshed: {} stations, {} channels",
                index.stations.len(),
                index.channels.len(),
            ),
        );
        *self.index.lock().unwrap() = Some(CachedIndex {
            fetched_at: Instant::now(),
            index: index.clone(),
        });
        Ok(index)
    }

    fn channel_ids(index: &EcoIndex) -> Vec<String> {
        index
            .channels
            .iter()
            .map(|channel| channel.external_id.clone())
            .collect()
    }

    fn effective_start(&self, from: Option<DateTime<Utc>>, _index: &EcoIndex) -> DateTime<Utc> {
        match from {
            Some(from) => from,
            None => {
                let lookback = Utc::now() - chrono::Duration::days(self.import_days_back);
                utc_midnight(lookback.date_naive())
            }
        }
    }

    fn ensure_scanner(
        &self,
        from: Option<DateTime<Utc>>,
        index: &EcoIndex,
    ) -> Result<(), ProviderError> {
        let ids = Self::channel_ids(index);
        let mut guard = self.scanner.lock().unwrap();
        let needs_seed = match guard.as_ref() {
            Some(state) => state.anchor != from || state.ids != ids,
            None => true,
        };
        if !needs_seed {
            return Ok(());
        }
        let seed = self.effective_start(from, index);
        *guard = Some(ScannerState {
            anchor: from,
            ids: ids.clone(),
            scanner: SourceScanner::new(Some(seed), &ids),
        });
        Ok(())
    }

    fn fill_channel_page(
        &self,
        external_id: &str,
        lower: DateTime<Utc>,
    ) -> Result<ChannelPage, ProviderError> {
        let index = self.ensure_index()?;
        let site = index.sites.get(external_id).ok_or_else(|| {
            ProviderError::InvalidData(format!("unknown eco-counter v1 station '{external_id}'"))
        })?;
        let id = external_id.parse::<i64>().map_err(|_| {
            ProviderError::InvalidData(format!("invalid eco-counter v1 station id '{external_id}'"))
        })?;
        let now = Utc::now();

        let begin_day = lower.date_naive();
        let Some(end_day) = begin_day.checked_add_days(Days::new(self.page_days as u64)) else {
            return Err(ProviderError::InvalidData(format!(
                "date overflow paging station '{external_id}'"
            )));
        };

        let rows = self.client.data(
            id,
            site.domain,
            &site.token,
            self.step,
            utc_midnight(begin_day),
            utc_midnight(end_day),
        )?;

        let mut measurements: Vec<SourceMeasurement> = Vec::new();
        let mut last_real: Option<DateTime<Utc>> = None;
        for row in &rows {
            let Some(record) = parse_row(row, self.resolution_seconds) else {
                continue;
            };
            // Exclusive lower bound; never import rows beyond the present.
            if record.timestamp <= lower || record.timestamp > now {
                continue;
            }
            if last_real.is_none_or(|t| record.timestamp > t) {
                last_real = Some(record.timestamp);
            }
            measurements.push(SourceMeasurement {
                channel_external_id: external_id.to_string(),
                record,
            });
        }

        // Done when the window covers at least up to today.
        let done = end_day > now.date_naive();
        let next_from = if done {
            None
        } else {
            Some(utc_midnight(end_day))
        };
        Ok(ChannelPage {
            measurements,
            last_real,
            next_from,
            done,
        })
    }
}

impl DataProvider for EcoCounterV1Provider {
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
        _max_batch_size: usize,
    ) -> Result<SourceMeasurementBatch, ProviderError> {
        let index = self.ensure_index()?;
        self.ensure_scanner(from, &index)?;

        let (external_id, lower) = {
            let mut guard = self.scanner.lock().unwrap();
            let state = guard.as_mut().expect("scanner seeded");
            match state.scanner.next_channel() {
                Some((id, Some(lower))) => (id, lower),
                Some((id, None)) => {
                    let seed = self.effective_start(from, &index);
                    (id, seed)
                }
                None => {
                    return Ok(SourceMeasurementBatch {
                        measurements: vec![],
                        next_from: None,
                        more: false,
                    });
                }
            }
        };

        let page = self.fill_channel_page(&external_id, lower)?;
        let mut guard = self.scanner.lock().unwrap();
        let state = guard.as_mut().expect("scanner seeded");
        state.scanner.record(&external_id, page)
    }

    fn max_measurement_batch_size(&self) -> usize {
        self.max_measurement_batch_size
    }

    fn attach_provider_messages(&self, sink: Arc<dyn ProviderMessageSink + Send + Sync>) {
        *self.messages.lock().unwrap() = Some(sink);
    }
}
