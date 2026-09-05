//! The **V2 Eco-Counter adapter** (provider type `eco_counter_v2_http_provider`):
//! imports stations from the **official Eco-Counter API** with an OAuth access
//! token. Stations are discovered at runtime from `/site` (optionally restricted
//! to one `domain_id`), so no per-station catalog is needed; each site's time
//! series is paged from `/data/site/{id}` over day windows.

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

use super::client::{DEFAULT_BASE_URL, OfficialApiClient};
use super::parsing::{V2Index, build_index, parse_point, resolution_for_step, step_token};

/// The provider type this adapter is registered under in the config.
pub(crate) const PROVIDER_TYPE: &str = "eco_counter_v2_http_provider";
pub(crate) const DEFAULT_MAX_MEASUREMENT_BATCH_SIZE: usize = 500;
pub(crate) const DEFAULT_CACHE_DURATION_SECS: u64 = 300;
/// Default numeric `step` (3 = hourly).
pub(crate) const DEFAULT_STEP: i64 = 3;
pub(crate) const DEFAULT_PAGE_DAYS: i64 = 7;
pub(crate) const DEFAULT_IMPORT_DAYS_BACK: i64 = 365;

pub struct EcoCounterV2Adapter {
    base_url: String,
    domain_id: Option<i64>,
    step: i64,
    step_token: &'static str,
    resolution_seconds: i64,
    max_measurement_batch_size: usize,
    cache_duration: Duration,
    page_days: i64,
    import_days_back: i64,
    client: OfficialApiClient,
    /// Scoped provider-message sink, attached by `StartupService`.
    messages: Mutex<Option<Arc<dyn ProviderMessageSink + Send + Sync>>>,
    /// In-memory cache of the discovered index.
    index: Mutex<Option<CachedIndex>>,
    refresh_lock: Mutex<()>,
    scanner: Mutex<Option<ScannerState>>,
}

struct CachedIndex {
    fetched_at: Instant,
    index: Arc<V2Index>,
}

struct ScannerState {
    anchor: Option<DateTime<Utc>>,
    ids: Vec<String>,
    scanner: SourceScanner,
}

impl EcoCounterV2Adapter {
    /// The provider type this adapter is registered under in the config.
    pub fn provider_type() -> &'static str {
        PROVIDER_TYPE
    }

    /// Builds the adapter from the data source's provider vars.
    ///
    /// Required var: `access_token` (the organisation's OAuth access token).
    /// Optional vars: `domain_id` (restrict discovery to one domain), `base_url`,
    /// `step` (`2` = 15 min, `3` = hourly, `4` = daily; default `3`),
    /// `max_measurement_batch_size`, `cache_duration`, `page_days`,
    /// `import_days_back`. Missing/invalid values are configuration errors.
    pub fn new(config: &DataSourceConfiguration) -> Result<Self, ConfigError> {
        let access_token = config
            .provider()
            .var("access_token")
            .map(str::to_string)
            .filter(|t| !t.is_empty())
            .ok_or_else(|| {
                ConfigError::InvalidFormat(format!(
                    "{PROVIDER_TYPE}: var 'access_token' is required (the organisation's \
                     Eco-Counter access token)"
                ))
            })?;
        Self::with_fetcher(
            config,
            Arc::new(HttpResourceFetcher::with_bearer(access_token)),
        )
    }

    pub(crate) fn with_fetcher(
        config: &DataSourceConfiguration,
        fetcher: Arc<dyn ResourceFetcher>,
    ) -> Result<Self, ConfigError> {
        let base_url = config
            .provider()
            .var("base_url")
            .unwrap_or(DEFAULT_BASE_URL)
            .trim_end_matches('/')
            .to_string();
        if base_url.is_empty() {
            return Err(ConfigError::InvalidFormat(format!(
                "{PROVIDER_TYPE}: var 'base_url' must not be empty"
            )));
        }

        let domain_id = match config.provider().var("domain_id") {
            Some(raw) => Some(raw.parse::<i64>().map_err(|_| {
                ConfigError::InvalidFormat(format!(
                    "{PROVIDER_TYPE}: var 'domain_id' is not a number"
                ))
            })?),
            None => None,
        };

        let step = match config.provider().var("step") {
            Some(raw) => raw.parse::<i64>().map_err(|_| {
                ConfigError::InvalidFormat(format!(
                    "{PROVIDER_TYPE}: var 'step' is not a valid number"
                ))
            })?,
            None => DEFAULT_STEP,
        };
        let step_token = step_token(step).ok_or_else(|| {
            ConfigError::InvalidFormat(format!(
                "{PROVIDER_TYPE}: var 'step' must be 2 (15 min), 3 (hourly) or 4 (daily)"
            ))
        })?;
        let resolution_seconds = resolution_for_step(step).expect("validated step");

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

        let page_days = match config.provider().var("page_days") {
            Some(raw) => raw.parse::<i64>().map_err(|_| {
                ConfigError::InvalidFormat(format!(
                    "{PROVIDER_TYPE}: var 'page_days' is not a valid number"
                ))
            })?,
            None => DEFAULT_PAGE_DAYS,
        }
        .max(1);

        let import_days_back = match config.provider().var("import_days_back") {
            Some(raw) => raw.parse::<i64>().map_err(|_| {
                ConfigError::InvalidFormat(format!(
                    "{PROVIDER_TYPE}: var 'import_days_back' is not a valid number"
                ))
            })?,
            None => DEFAULT_IMPORT_DAYS_BACK,
        }
        .max(1);

        Ok(Self {
            base_url: base_url.clone(),
            domain_id,
            step,
            step_token,
            resolution_seconds,
            max_measurement_batch_size,
            cache_duration: Duration::from_secs(cache_duration),
            page_days,
            import_days_back,
            client: OfficialApiClient::new(base_url, fetcher),
            messages: Mutex::new(None),
            index: Mutex::new(None),
            refresh_lock: Mutex::new(()),
            scanner: Mutex::new(None),
        })
    }

    pub fn step(&self) -> i64 {
        self.step
    }

    pub fn cache_duration_secs(&self) -> u64 {
        self.cache_duration.as_secs()
    }

    fn messages_sink(&self) -> Option<Arc<dyn ProviderMessageSink + Send + Sync>> {
        self.messages.lock().unwrap().clone()
    }

    fn emit(&self, severity: ProviderMessageSeverity, message: impl AsRef<str>) {
        if let Some(sink) = self.messages.lock().unwrap().as_ref() {
            let _ = sink.provider_event_occurred(severity, message.as_ref());
        }
    }

    /// Ensures a parsed index exists (refreshes the `/site` discovery when the
    /// cache expired).
    fn ensure_index(&self) -> Result<Arc<V2Index>, ProviderError> {
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

        let sites = self.client.sites(self.domain_id)?;
        if sites.is_empty() {
            return Err(ProviderError::Unreachable(
                "eco-counter v2: no sites returned (is the access token valid and scoped to a \
                 domain?)"
                    .to_string(),
            ));
        }
        let index = Arc::new(build_index(&sites));
        self.emit(
            ProviderMessageSeverity::Info,
            format!(
                "eco-counter v2 discovery refreshed: {} stations, {} channels",
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

    fn channel_ids(index: &V2Index) -> Vec<String> {
        index
            .channels
            .iter()
            .map(|channel| channel.external_id.clone())
            .collect()
    }

    fn effective_start(&self, from: Option<DateTime<Utc>>, _index: &V2Index) -> DateTime<Utc> {
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
        index: &V2Index,
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
        let id = external_id.parse::<i64>().map_err(|_| {
            ProviderError::InvalidData(format!("invalid eco-counter v2 site id '{external_id}'"))
        })?;
        let now = Utc::now();

        let begin_day = lower.date_naive();
        let Some(end_day) = begin_day.checked_add_days(Days::new(self.page_days as u64)) else {
            return Err(ProviderError::InvalidData(format!(
                "date overflow paging site '{external_id}'"
            )));
        };

        let points = self.client.data(
            id,
            self.step_token,
            utc_midnight(begin_day),
            utc_midnight(end_day),
        )?;

        let mut measurements: Vec<SourceMeasurement> = Vec::new();
        let mut last_real: Option<DateTime<Utc>> = None;
        for point in &points {
            let Some(record) = parse_point(point, self.resolution_seconds) else {
                continue;
            };
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

impl DataProvider for EcoCounterV2Adapter {
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
