//! The **Eco-Counter web adapter** (provider type `eco_counter_web_http_provider`):
//! imports the daily counts of the public Eco-Counter dashboards
//! (`*.eco-counter.com`) that have no usable API.
//!
//! The scraper reads the Next.js RSC payloads (see [`super::parsing`]):
//!
//! - the **home page** embeds the tenant's full station list (`sites[]`), so no
//!   per-station catalog is needed;
//! - each **detail page** embeds that site's **daily** series for one calendar
//!   year (`?granularity=P1D&year=YYYY`), which is paged year by year over the
//!   shared [`SourceScanner`](crate::adapter::driven::source_merge).
//!
//! Each station maps to **one** channel carrying the site-total daily count. The
//! discovery result is cached in memory *and* through the scoped
//! [`PersistentStateAccess`] handle, so a restart does not re-scrape the big
//! station list. Requests are rate-limited (default 1/second).

use std::net::TcpStream;
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use chrono::{DateTime, Datelike, Days, NaiveDate, TimeZone, Utc};
use chrono_tz::Tz;
use serde::{Deserialize, Serialize};

use crate::adapter::driven::eco_counter::common::parse_host_and_port;
use crate::adapter::driven::source_merge::{ChannelPage, SourceScanner};
use crate::core::domain::configuration::configuration::value_objects::DataSourceConfiguration;
use crate::core::domain::configuration::error::ConfigError;
use crate::core::domain::data_source::provider_message::ProviderMessageSeverity;
use crate::core::domain::data_source::provider_port::{
    ChannelRecord, CountingStationRecord, DataProvider, PersistentStateAccess, ProviderError,
    ProviderMessageSink, SourceMeasurement, SourceMeasurementBatch,
};
use crate::core::domain::health::HealthStatus;

use super::client::PageClient;
use super::fetcher::{HttpPageFetcher, PageFetcher};
use super::parsing::{DailyValue, SiteIndex, parse_daily_series, parse_site_list};
use super::rate_limit::RateLimiter;

/// The provider type this adapter is registered under in the config.
pub(crate) const PROVIDER_TYPE: &str = "eco_counter_web_http_provider";
/// Seconds per **daily** measurement.
pub const DAY_SECONDS: i64 = 86_400;

pub(crate) const DEFAULT_MAX_MEASUREMENT_BATCH_SIZE: usize = 500;
pub(crate) const DEFAULT_CACHE_DURATION_SECS: u64 = 300;
/// Default initial lookback (days) when no `imported_until` watermark exists.
pub(crate) const DEFAULT_IMPORT_DAYS_BACK: i64 = 365;
/// Default rate limit: one request per second.
pub(crate) const DEFAULT_REQUESTS_PER_SECOND: f64 = 1.0;
/// Default IANA timezone of the station daily series.
pub(crate) const DEFAULT_TIMEZONE: &str = "Europe/Berlin";

/// Persistent-state keys owned by this adapter (scoped to the data source).
pub(crate) const KEY_INDEX: &str = "index";
pub(crate) const KEY_INDEX_AT: &str = "index_at";

/// A serialisable copy of the served station/channel index (persistent state).
#[derive(Debug, Serialize, Deserialize)]
struct StoredIndex {
    timezone: String,
    stations: Vec<StoredStation>,
    channels: Vec<StoredChannel>,
}

#[derive(Debug, Serialize, Deserialize)]
struct StoredStation {
    external_id: String,
    name: String,
    description: String,
    latitude: Option<f64>,
    longitude: Option<f64>,
}

#[derive(Debug, Serialize, Deserialize)]
struct StoredChannel {
    external_id: String,
    station_external_id: String,
    name: String,
}

pub struct EcoCounterWebAdapter {
    base_url: String,
    timezone: Tz,
    max_measurement_batch_size: usize,
    cache_duration: Duration,
    import_days_back: i64,
    client: PageClient,
    /// Scoped provider-message sink, attached by `StartupService`.
    messages: Mutex<Option<Arc<dyn ProviderMessageSink + Send + Sync>>>,
    /// Scoped persistent-state handle (two-phase handover), attached after the
    /// data source is persisted. `None` until attached.
    state: Mutex<Option<Arc<dyn PersistentStateAccess + Send + Sync>>>,
    /// In-memory cache of the parsed station index.
    index: Mutex<Option<CachedIndex>>,
    refresh_lock: Mutex<()>,
    /// Whole-source (channel-interleaved) reader state for the current run.
    scanner: Mutex<Option<ScannerState>>,
}

struct CachedIndex {
    fetched_at: Instant,
    index: Arc<SiteIndex>,
}

/// Per-run measurement reader state: the run anchor plus a fair round-robin
/// scanner over the site ids.
struct ScannerState {
    anchor: Option<DateTime<Utc>>,
    ids: Vec<String>,
    scanner: SourceScanner,
}

impl EcoCounterWebAdapter {
    /// The provider type this adapter is registered under in the config.
    pub fn provider_type() -> &'static str {
        PROVIDER_TYPE
    }

    /// Builds the adapter from the data source's provider vars.
    ///
    /// Required var: `scrape_url` (the tenant root, e.g.
    /// `https://hessen-mobil.eco-counter.com`). Optional vars: `timezone`
    /// (default `Europe/Berlin`), `cache_duration` (seconds, default `300`),
    /// `import_days_back` (default `365`),
    /// `rate_limit_requests_per_second` (default `1`),
    /// `max_measurement_batch_size` (default `500`).
    pub fn new(config: &DataSourceConfiguration) -> Result<Self, ConfigError> {
        Self::with_fetcher(config, Arc::new(HttpPageFetcher))
    }

    pub(crate) fn with_fetcher(
        config: &DataSourceConfiguration,
        fetcher: Arc<dyn PageFetcher>,
    ) -> Result<Self, ConfigError> {
        let scrape_url = config
            .provider()
            .var("scrape_url")
            .map(str::trim)
            .filter(|url| !url.is_empty())
            .ok_or_else(|| {
                ConfigError::InvalidFormat(format!(
                    "{PROVIDER_TYPE}: var 'scrape_url' is required (the tenant root, e.g. \
                     https://hessen-mobil.eco-counter.com)"
                ))
            })?
            .trim_end_matches('/')
            .to_string();

        let timezone = parse_timezone(config.provider().var("timezone"))?;
        let import_days_back = parse_i64(
            config.provider().var("import_days_back"),
            DEFAULT_IMPORT_DAYS_BACK,
        )
        .map_err(|_| {
            ConfigError::InvalidFormat(format!(
                "{PROVIDER_TYPE}: var 'import_days_back' is not a valid number"
            ))
        })?
        .max(1);
        let cache_duration = parse_u64(
            config.provider().var("cache_duration"),
            DEFAULT_CACHE_DURATION_SECS,
        )
        .map_err(|_| {
            ConfigError::InvalidFormat(format!(
                "{PROVIDER_TYPE}: var 'cache_duration' is not a valid number"
            ))
        })?;
        let requests_per_second = parse_f64(
            config.provider().var("rate_limit_requests_per_second"),
            DEFAULT_REQUESTS_PER_SECOND,
        )
        .map_err(|_| {
            ConfigError::InvalidFormat(format!(
                "{PROVIDER_TYPE}: var 'rate_limit_requests_per_second' is not a valid number"
            ))
        })?
        .max(0.0);
        let max_measurement_batch_size = parse_usize(
            config.provider().var("max_measurement_batch_size"),
            DEFAULT_MAX_MEASUREMENT_BATCH_SIZE,
        )
        .map_err(|_| {
            ConfigError::InvalidFormat(format!(
                "{PROVIDER_TYPE}: var 'max_measurement_batch_size' is not a valid number"
            ))
        })?;

        Ok(Self {
            base_url: scrape_url.clone(),
            timezone,
            max_measurement_batch_size,
            cache_duration: Duration::from_secs(cache_duration),
            import_days_back,
            client: PageClient::new(scrape_url, fetcher, RateLimiter::new(requests_per_second)),
            messages: Mutex::new(None),
            state: Mutex::new(None),
            index: Mutex::new(None),
            refresh_lock: Mutex::new(()),
            scanner: Mutex::new(None),
        })
    }

    fn messages_sink(&self) -> Option<Arc<dyn ProviderMessageSink + Send + Sync>> {
        self.messages.lock().unwrap().clone()
    }

    fn emit(&self, severity: ProviderMessageSeverity, message: impl AsRef<str>) {
        if let Some(sink) = self.messages.lock().unwrap().as_ref() {
            let _ = sink.provider_event_occurred(severity, message.as_ref());
        }
    }

    /// The UTC instant of `day` at **local midnight** in the configured
    /// timezone — every daily bucket of the source starts on such an instant.
    fn local_midnight(&self, day: NaiveDate) -> DateTime<Utc> {
        let naive = day
            .and_hms_opt(0, 0, 0)
            .expect("midnight of any calendar day exists");
        self.timezone
            .from_local_datetime(&naive)
            .single()
            .expect("local midnight is never ambiguous in the configured timezone")
            .with_timezone(&Utc)
    }

    /// The UTC instant of the local midnight that *starts* the current calendar
    /// day (rows on/after it are not complete and are never imported).
    fn today_start(&self) -> DateTime<Utc> {
        let today = Utc::now().with_timezone(&self.timezone).date_naive();
        self.local_midnight(today)
    }

    // -- discovery -----------------------------------------------------------

    /// Ensures a usable station index exists (fresh in-memory cache, a fresh
    /// persisted index, or a live home-page fetch).
    fn ensure_index(&self) -> Result<Arc<SiteIndex>, ProviderError> {
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

        // A freshly persisted index (from a previous run) avoids re-scraping the
        // big station list right after a restart.
        if let Some(index) = self.load_persisted_index()? {
            self.emit(
                ProviderMessageSeverity::Info,
                format!(
                    "eco-counter web: reused cached discovery ({} stations)",
                    index.stations.len()
                ),
            );
            *self.index.lock().unwrap() = Some(CachedIndex {
                fetched_at: Instant::now(),
                index: index.clone(),
            });
            return Ok(index);
        }

        let year = Utc::now().with_timezone(&self.timezone).year();
        let payload = self.client.fetch_stations(year).map_err(|message| {
            ProviderError::Unreachable(format!(
                "eco-counter web: station list fetch failed: {message}"
            ))
        })?;
        let messages = self.messages_sink();
        let index = Arc::new(
            parse_site_list(&payload, &self.timezone.to_string(), messages.as_deref())
                .map_err(ProviderError::InvalidData)?,
        );
        if index.stations.is_empty() {
            return Err(ProviderError::Unreachable(
                "eco-counter web: no bicycle counting sites found in the station list".to_string(),
            ));
        }

        self.emit(
            ProviderMessageSeverity::Info,
            format!(
                "eco-counter web discovery refreshed: {} stations, {} channels",
                index.stations.len(),
                index.channels.len(),
            ),
        );
        let _ = self.save_persisted_index(&index); // cache only; never fatal
        *self.index.lock().unwrap() = Some(CachedIndex {
            fetched_at: Instant::now(),
            index: index.clone(),
        });
        Ok(index)
    }

    fn load_persisted_index(&self) -> Result<Option<Arc<SiteIndex>>, ProviderError> {
        let state = self.state.lock().unwrap().clone();
        let Some(state) = state else {
            return Ok(None);
        };
        let map = state.load()?;
        let Some(at) = map
            .get(KEY_INDEX_AT)
            .and_then(|raw| DateTime::parse_from_rfc3339(raw).ok())
            .map(|dt| dt.with_timezone(&Utc))
        else {
            return Ok(None);
        };
        // Only reuse an index fetched within the cache window.
        if at + chrono::Duration::seconds(self.cache_duration.as_secs() as i64) < Utc::now() {
            return Ok(None);
        }
        let Some(json) = map.get(KEY_INDEX) else {
            return Ok(None);
        };
        match decode_index(json) {
            Ok(index) => Ok(Some(Arc::new(index))),
            Err(_) => {
                // Corrupt/legacy payload: drop it and re-scrape.
                let _ = state.delete(KEY_INDEX);
                let _ = state.delete(KEY_INDEX_AT);
                Ok(None)
            }
        }
    }

    fn save_persisted_index(&self, index: &SiteIndex) -> Result<(), ProviderError> {
        if let Some(state) = self.state.lock().unwrap().clone() {
            let json = encode_index(index);
            state.store(KEY_INDEX, &json)?;
            state.store(KEY_INDEX_AT, &Utc::now().to_rfc3339())?;
        }
        Ok(())
    }

    // -- measurements --------------------------------------------------------

    fn effective_start(&self, from: Option<DateTime<Utc>>) -> DateTime<Utc> {
        match from {
            Some(from) => from,
            None => {
                let lookback = Utc::now() - chrono::Duration::days(self.import_days_back);
                let day = lookback.with_timezone(&self.timezone).date_naive();
                self.local_midnight(day)
            }
        }
    }

    fn ensure_scanner(
        &self,
        from: Option<DateTime<Utc>>,
        index: &SiteIndex,
    ) -> Result<(), ProviderError> {
        let ids = index.site_ids.clone();
        let mut guard = self.scanner.lock().unwrap();
        let needs_seed = match guard.as_ref() {
            Some(state) => state.anchor != from || state.ids != ids,
            None => true,
        };
        if !needs_seed {
            return Ok(());
        }
        let seed = self.effective_start(from);
        *guard = Some(ScannerState {
            anchor: from,
            ids: ids.clone(),
            scanner: SourceScanner::new(Some(seed), &ids),
        });
        Ok(())
    }

    /// Fetches the next **calendar year** of a site's daily series (one page).
    fn fill_site_page(
        &self,
        external_id: &str,
        lower: DateTime<Utc>,
    ) -> Result<ChannelPage, ProviderError> {
        // The first bucket *after* `lower` decides which year to fetch, so a
        // completed year whose final day is `lower` moves straight on to the next.
        let lower_day = lower.with_timezone(&self.timezone).date_naive();
        let Some(next_day) = lower_day.checked_add_days(Days::new(1)) else {
            return Err(ProviderError::InvalidData(format!(
                "date overflow paging site '{external_id}'"
            )));
        };
        let year = next_day.year();
        let today_start = self.today_start();

        let payload = self
            .client
            .fetch_site_data(external_id, year)
            .map_err(|message| {
                ProviderError::Unreachable(format!(
                    "eco-counter web: daily fetch failed (site {external_id}, {year}): \
                     {message}"
                ))
            })?;
        let values =
            parse_daily_series(&payload, &self.timezone).map_err(ProviderError::InvalidData)?;

        let mut measurements: Vec<SourceMeasurement> = Vec::new();
        let mut last_real: Option<DateTime<Utc>> = None;
        for value in values {
            if let Some(record) = self.to_daily_record(&value, lower, today_start)? {
                if last_real.is_none_or(|t| value.timestamp > t) {
                    last_real = Some(value.timestamp);
                }
                measurements.push(SourceMeasurement {
                    channel_external_id: external_id.to_string(),
                    record,
                });
            }
        }

        // Done once the fetched year is the current one: all complete days of
        // the source are imported.
        let now_year = Utc::now().with_timezone(&self.timezone).year();
        let done = year >= now_year;
        let next_from = if done {
            None
        } else {
            // Advance to the following calendar year: after a real last day, the
            // next page resolves the following year from it; a data-less year
            // skips ahead with a synthetic cursor (never a watermark).
            match last_real {
                Some(last) => Some(last),
                None => {
                    // No real row in the fetched year: jump to its final day so
                    // the next page computes `year + 1`.
                    let final_day = NaiveDate::from_ymd_opt(year, 12, 31).ok_or_else(|| {
                        ProviderError::InvalidData(format!(
                            "invalid end of year {year} for site '{external_id}'"
                        ))
                    })?;
                    Some(self.local_midnight(final_day))
                }
            }
        };

        Ok(ChannelPage {
            measurements,
            last_real,
            next_from,
            done,
        })
    }

    /// Converts one daily value into a daily measurement strictly inside
    /// `(lower, today_start)` — complete calendar days only.
    fn to_daily_record(
        &self,
        value: &DailyValue,
        lower: DateTime<Utc>,
        today_start: DateTime<Utc>,
    ) -> Result<
        Option<crate::core::domain::data_source::provider_port::MeasurementRecord>,
        ProviderError,
    > {
        if value.timestamp <= lower || value.timestamp >= today_start {
            return Ok(None);
        }
        let day = value.timestamp.with_timezone(&self.timezone).date_naive();
        let Some(next_day) = day.checked_add_days(Days::new(1)) else {
            return Ok(None);
        };
        let interval_end = self.local_midnight(next_day);
        Ok(Some(
            crate::core::domain::data_source::provider_port::MeasurementRecord {
                value: value.value,
                timestamp: value.timestamp,
                resolution_seconds: DAY_SECONDS,
                interval_end: Some(interval_end),
            },
        ))
    }
}

impl DataProvider for EcoCounterWebAdapter {
    fn check_health(&self) -> HealthStatus {
        let Some((host, port)) = parse_host_and_port(&self.base_url) else {
            return HealthStatus::Down("invalid scrape_url in provider config".to_string());
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
                    let seed = self.effective_start(from);
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

        let page = self.fill_site_page(&external_id, lower)?;
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

    fn attach_persistent_state(&self, state: Arc<dyn PersistentStateAccess + Send + Sync>) {
        *self.state.lock().unwrap() = Some(state);
    }
}

// -- config helpers ----------------------------------------------------------

fn parse_timezone(raw: Option<&str>) -> Result<Tz, ConfigError> {
    let value = raw.unwrap_or(DEFAULT_TIMEZONE);
    value.parse::<Tz>().map_err(|_| {
        ConfigError::InvalidFormat(format!(
            "{PROVIDER_TYPE}: var 'timezone' is not a valid IANA timezone ('{value}')"
        ))
    })
}

fn parse_u64(raw: Option<&str>, default: u64) -> Result<u64, ()> {
    match raw {
        Some(value) => value.parse::<u64>().map_err(|_| ()),
        None => Ok(default),
    }
}

fn parse_i64(raw: Option<&str>, default: i64) -> Result<i64, ()> {
    match raw {
        Some(value) => value.parse::<i64>().map_err(|_| ()),
        None => Ok(default),
    }
}

fn parse_f64(raw: Option<&str>, default: f64) -> Result<f64, ()> {
    match raw {
        Some(value) => value.parse::<f64>().map_err(|_| ()),
        None => Ok(default),
    }
}

fn parse_usize(raw: Option<&str>, default: usize) -> Result<usize, ()> {
    match raw {
        Some(value) => value.parse::<usize>().map_err(|_| ()),
        None => Ok(default),
    }
}

// -- index persistence -------------------------------------------------------

fn encode_index(index: &SiteIndex) -> String {
    let stored = StoredIndex {
        timezone: index
            .stations
            .first()
            .map(|station| station.timezone.clone())
            .unwrap_or_default(),
        stations: index
            .stations
            .iter()
            .map(|station| StoredStation {
                external_id: station.external_id.clone(),
                name: station.name.clone(),
                description: station.description.clone(),
                latitude: station.latitude,
                longitude: station.longitude,
            })
            .collect(),
        channels: index
            .channels
            .iter()
            .map(|channel| StoredChannel {
                external_id: channel.external_id.clone(),
                station_external_id: channel.counting_station_external_id.clone(),
                name: channel.name.clone(),
            })
            .collect(),
    };
    serde_json::to_string(&stored).expect("serialisable index")
}

fn decode_index(json: &str) -> Result<SiteIndex, ProviderError> {
    let stored: StoredIndex = serde_json::from_str(json).map_err(|error| {
        ProviderError::Storage(format!(
            "screen scraping: cannot decode cached index: {error}"
        ))
    })?;
    let timezone = stored.timezone;
    let mut index = SiteIndex::default();
    index.stations = stored
        .stations
        .into_iter()
        .map(|station| CountingStationRecord {
            external_id: station.external_id,
            name: station.name,
            description: station.description,
            latitude: station.latitude,
            longitude: station.longitude,
            timezone: timezone.clone(),
            image_sha256: None,
        })
        .collect();
    index.channels = stored
        .channels
        .into_iter()
        .map(|channel| ChannelRecord {
            external_id: channel.external_id,
            counting_station_external_id: channel.station_external_id,
            name: channel.name,
            description: String::new(),
        })
        .collect();
    index.site_ids = index
        .channels
        .iter()
        .map(|channel| channel.external_id.clone())
        .collect();
    index
        .stations
        .sort_by(|a, b| a.external_id.cmp(&b.external_id));
    index
        .channels
        .sort_by(|a, b| a.external_id.cmp(&b.external_id));
    index.site_ids.sort();
    Ok(index)
}
