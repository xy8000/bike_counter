//! Data provider for the Münster open-data GitHub archive.
//!
//! Downloads the configured ZIP, extracts it into an obscured `/tmp` folder,
//! and serves counting stations, channels and measurements from the extracted
//! files. Cache metadata is kept through the scoped [`PersistentStateAccess`]
//! handle; all data-serving methods are synchronous so they can run inside
//! `spawn_blocking`.

use std::collections::HashMap;
use std::fs::{self, File};
use std::io::{BufWriter, Write};
use std::net::TcpStream;
use std::path::{Component, Path, PathBuf};
use std::sync::{Arc, Mutex};

use chrono::{DateTime, Duration, NaiveDate, NaiveDateTime, TimeZone, Utc};
use chrono_tz::Europe::Berlin;
use chrono_tz::Tz;
use uuid::Uuid;

use crate::core::domain::configuration::configuration::value_objects::DataSourceConfiguration;
use crate::core::domain::configuration::error::ConfigError;
use crate::core::domain::data_source::provider::{
    ChannelRecord, CountingStationRecord, DataProvider, MeasurementBatch, MeasurementQuery,
    MeasurementRecord, PersistentStateAccess, ProviderError, ProviderMessageSink,
};
use crate::core::domain::data_source::provider_message::ProviderMessageSeverity;
use crate::core::domain::health::HealthStatus;

const PROVIDER_TYPE: &str = "münster_opendata_github_provider";
const DEFAULT_MAX_MEASUREMENT_BATCH_SIZE: usize = 500;
/// Default import time window per provider call: 7 days, in hours.
const DEFAULT_MAX_MEASUREMENT_TIMEFRAME_HOURS: u64 = 168;
const DEFAULT_CACHE_DURATION_SECS: u64 = 300;

/// Persistent-state keys owned by this provider.
const KEY_DOWNLOADED_AT: &str = "archive_downloaded_at";
const KEY_EXTRACTED_AT: &str = "archive_extracted_at";
const KEY_ARCHIVE_FILE: &str = "archive_file";
const KEY_EXTRACTED_DIR: &str = "archive_extracted_dir";
const KEY_ETAG: &str = "archive_etag";
const KEY_LAST_MODIFIED: &str = "archive_last_modified";

/// Archive internals (verified against the example archive).
const ARCHIVE_ROOT: &str = "radverkehr-zaehlstellen-main";
const SITE_INDEX_FILE: &str = "site_min.json";

/// Timezone the raw CSVs are written in.
const TIMEZONE: Tz = Berlin;

// ---------------------------------------------------------------------------
// HTTP abstraction (so cache tiers are testable without a network).
// ---------------------------------------------------------------------------

/// Headers returned by the upstream that are used for change detection.
#[derive(Debug, Clone, Default)]
struct UpstreamHeaders {
    etag: Option<String>,
    last_modified: Option<String>,
}

impl UpstreamHeaders {
    fn is_empty(&self) -> bool {
        self.etag.is_none() && self.last_modified.is_none()
    }

    /// True when this header set equals `other` (ETag preferred, fall back to
    /// Last-Modified).
    fn matches(&self, other: &UpstreamHeaders) -> bool {
        match (&self.etag, &other.etag) {
            (Some(a), Some(b)) => a == b,
            _ => match (&self.last_modified, &other.last_modified) {
                (Some(a), Some(b)) => a == b,
                _ => false,
            },
        }
    }
}

/// Fetches the archive over HTTP. Real implementation is [`HttpFetcher`]; tests
/// inject a fake.
trait ArchiveFetcher: Send + Sync {
    /// Best-effort `HEAD` request. `None` when no headers are available.
    fn head(&self, url: &str) -> Option<UpstreamHeaders>;
    /// Downloads the archive body to `target`, returning the response headers.
    fn get(&self, url: &str, target: &Path) -> Result<UpstreamHeaders, String>;
}

struct HttpFetcher;

impl ArchiveFetcher for HttpFetcher {
    fn head(&self, url: &str) -> Option<UpstreamHeaders> {
        let response = ureq::head(url).call().ok()?;
        Some(UpstreamHeaders {
            etag: response.header("ETag").map(str::to_string),
            last_modified: response.header("Last-Modified").map(str::to_string),
        })
    }

    fn get(&self, url: &str, target: &Path) -> Result<UpstreamHeaders, String> {
        let response = ureq::get(url).call().map_err(|error| format!("{error}"))?;
        let headers = UpstreamHeaders {
            etag: response.header("ETag").map(str::to_string),
            last_modified: response.header("Last-Modified").map(str::to_string),
        };
        let mut reader = response.into_reader();
        let mut file = BufWriter::new(File::create(target).map_err(|e| e.to_string())?);
        std::io::copy(&mut reader, &mut file).map_err(|e| e.to_string())?;
        file.flush().map_err(|e| e.to_string())?;
        Ok(headers)
    }
}

// ---------------------------------------------------------------------------
// Parsed archive index.
// ---------------------------------------------------------------------------

/// In-memory representation of an extracted archive.
struct ArchiveIndex {
    stations: Vec<CountingStationRecord>,
    channels: Vec<ChannelRecord>,
    /// channel external id -> monthly CSV paths containing that channel.
    channel_csvs: HashMap<String, Vec<PathBuf>>,
    extracted_dir: PathBuf,
}

// ---------------------------------------------------------------------------
// Adapter.
// ---------------------------------------------------------------------------

pub struct MuensterGithubAdapter {
    url: String,
    max_measurement_batch_size: usize,
    /// Import time window per page; bounds how much history a single provider
    /// call reads regardless of the row-count batch size.
    max_measurement_timeframe: Duration,
    cache_duration: u64,
    fetcher: Arc<dyn ArchiveFetcher>,
    /// Scoped persistent-state handle, attached by `StartupService` after the
    /// data source is persisted (two-phase handover). `None` until attached.
    state: Mutex<Option<Arc<dyn PersistentStateAccess + Send + Sync>>>,
    /// Scoped provider-message sink, attached by `StartupService` after the data
    /// source is persisted. `None` until attached.
    messages: Mutex<Option<Arc<dyn ProviderMessageSink + Send + Sync>>>,
    /// Serializes cache refresh/extraction across threads.
    refresh_lock: Mutex<()>,
    /// In-memory index of the currently usable archive.
    index: Mutex<Option<Arc<ArchiveIndex>>>,
}

impl MuensterGithubAdapter {
    pub fn provider_type() -> &'static str {
        PROVIDER_TYPE
    }

    /// Builds the adapter from the data source's provider vars.
    ///
    /// Phase 1 of the two-phase handover: parses only static config and stores
    /// **no** state handle, so construction is DB-free. The handle is attached
    /// later via [`DataProvider::attach_persistent_state`].
    ///
    /// Required var: `url`. Optional vars: `max_measurement_batch_size`,
    /// `max_measurement_timeframe_hours` (hours, default `168`), `cache_duration`
    /// (seconds, default `300`). A missing/invalid value is a configuration
    /// error (blocks startup).
    pub fn new(config: &DataSourceConfiguration) -> Result<Self, ConfigError> {
        Self::with_fetcher(config, Arc::new(HttpFetcher))
    }

    fn with_fetcher(
        config: &DataSourceConfiguration,
        fetcher: Arc<dyn ArchiveFetcher>,
    ) -> Result<Self, ConfigError> {
        let url = config
            .provider()
            .var("url")
            .ok_or_else(|| {
                ConfigError::InvalidFormat(format!("{PROVIDER_TYPE}: missing required var 'url'"))
            })?
            .to_string();

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

        let timeframe_hours = match config.provider().var("max_measurement_timeframe_hours") {
            Some(raw) => raw.parse::<u64>().map_err(|_| {
                ConfigError::InvalidFormat(format!(
                    "{PROVIDER_TYPE}: var 'max_measurement_timeframe_hours' is not a valid number"
                ))
            })?,
            None => DEFAULT_MAX_MEASUREMENT_TIMEFRAME_HOURS,
        };

        Ok(Self {
            url,
            max_measurement_batch_size,
            max_measurement_timeframe: Duration::hours(timeframe_hours as i64),
            cache_duration,
            fetcher,
            state: Mutex::new(None),
            messages: Mutex::new(None),
            refresh_lock: Mutex::new(()),
            index: Mutex::new(None),
        })
    }

    /// The configured cache window in seconds.
    pub fn cache_duration(&self) -> u64 {
        self.cache_duration
    }

    /// The configured import time window in hours (test-only read path).
    pub fn max_measurement_timeframe_hours(&self) -> u64 {
        self.max_measurement_timeframe.num_hours() as u64
    }

    /// The attached persistent-state handle, if any (test-only accessor).
    #[cfg(test)]
    fn attached_state(&self) -> Option<Arc<dyn PersistentStateAccess + Send + Sync>> {
        self.state.lock().unwrap().clone()
    }

    // -- state helpers -------------------------------------------------------

    fn load_state(&self) -> Result<HashMap<String, String>, ProviderError> {
        match self.state.lock().unwrap().as_ref() {
            Some(state) => state.load(),
            None => Ok(HashMap::new()),
        }
    }

    fn store_state(&self, key: &str, value: &str) -> Result<(), ProviderError> {
        if let Some(state) = self.state.lock().unwrap().as_ref() {
            state.store(key, value)?;
        }
        Ok(())
    }

    /// The attached provider-message sink, if any (cloned handle for callers).
    fn messages_sink(&self) -> Option<Arc<dyn ProviderMessageSink + Send + Sync>> {
        self.messages.lock().unwrap().clone()
    }

    /// Emits a scoped provider message (best-effort). Recording a message is
    /// never fatal for the provider's data-serving work, so a store failure here
    /// is swallowed rather than propagated.
    fn emit(&self, severity: ProviderMessageSeverity, message: impl AsRef<str>) {
        if let Some(sink) = self.messages.lock().unwrap().as_ref() {
            let _ = sink.provider_event_occurred(severity, message.as_ref());
        }
    }

    /// True when the persisted extracted directory exists and is still inside
    /// the cache window.
    fn is_extracted_fresh(&self) -> Result<bool, ProviderError> {
        let state = self.load_state()?;
        let Some(extracted_at) = state.get(KEY_EXTRACTED_AT).and_then(|v| parse_rfc3339(v)) else {
            return Ok(false);
        };
        let Some(dir) = state.get(KEY_EXTRACTED_DIR) else {
            return Ok(false);
        };
        Ok(Path::new(dir).exists()
            && extracted_at + chrono::Duration::seconds(self.cache_duration as i64) >= Utc::now())
    }

    // -- cache lifecycle -----------------------------------------------------

    /// Ensures a usable extracted archive exists and returns its index.
    fn ensure_archive(&self) -> Result<Arc<ArchiveIndex>, ProviderError> {
        // Fast path: index present and archive still fresh.
        if let Some(index) = self.index.lock().unwrap().as_ref().cloned()
            && self.is_extracted_fresh()?
        {
            return Ok(index);
        }

        // Slow path: serialize the refresh, then re-check under the lock.
        let _guard = self
            .refresh_lock
            .lock()
            .map_err(|_| ProviderError::Storage("refresh lock poisoned".to_string()))?;
        if let Some(index) = self.index.lock().unwrap().as_ref().cloned()
            && self.is_extracted_fresh()?
        {
            return Ok(index);
        }

        self.refresh_archive()?;
        let index = self.build_index()?;
        *self.index.lock().unwrap() = Some(index.clone());
        Ok(index)
    }

    /// Applies the four-tier cache decision and leaves a usable extracted
    /// directory on disk.
    fn refresh_archive(&self) -> Result<(), ProviderError> {
        let state = self.load_state()?;
        let now = Utc::now();
        let cache = chrono::Duration::seconds(self.cache_duration as i64);

        let extracted_at = state.get(KEY_EXTRACTED_AT).and_then(|v| parse_rfc3339(v));
        let extracted_dir = state.get(KEY_EXTRACTED_DIR).map(PathBuf::from);
        let downloaded_at = state.get(KEY_DOWNLOADED_AT).and_then(|v| parse_rfc3339(v));
        let zip_path = state.get(KEY_ARCHIVE_FILE).map(PathBuf::from);

        // Tier 1: extracted folder fresh -> reuse.
        if let (Some(at), Some(dir)) = (extracted_at, extracted_dir)
            && at + cache >= now
            && dir.exists()
        {
            self.emit(
                ProviderMessageSeverity::Debug,
                "archive cache fresh: reusing extracted folder",
            );
            return Ok(());
        }

        // Tier 2: ZIP fresh but folder missing -> re-extract from the ZIP.
        if let (Some(at), Some(zip)) = (downloaded_at, zip_path.as_ref())
            && at + cache >= now
            && zip.exists()
        {
            let dir = self.extract(zip)?;
            self.store_extracted(&dir, now)?;
            self.emit(
                ProviderMessageSeverity::Info,
                "archive cache fresh: re-extracted from existing zip",
            );
            return Ok(());
        }

        // Tier 4: best-effort upstream-change detection. If the upstream
        // headers are unchanged, reuse the stale ZIP and just re-extract.
        let upstream = self.fetcher.head(&self.url);
        if let Some(zip) = zip_path.as_ref()
            && let Some(upstream) = upstream.as_ref()
        {
            let persisted = UpstreamHeaders {
                etag: state.get(KEY_ETAG).cloned(),
                last_modified: state.get(KEY_LAST_MODIFIED).cloned(),
            };
            if zip.exists() && upstream.matches(&persisted) {
                let dir = self.extract(zip)?;
                self.store_extracted(&dir, now)?;
                self.emit(
                    ProviderMessageSeverity::Info,
                    "upstream unchanged: re-extracted cached archive",
                );
                return Ok(());
            }
        }

        // Tier 3: (re-)download and extract.
        let (zip, headers) = self.download(now)?;
        let dir = self.extract(&zip)?;
        self.store_refreshed(&zip, &dir, now, &headers)?;
        Ok(())
    }

    /// Downloads the archive to a fresh obscured temp file.
    fn download(&self, now: DateTime<Utc>) -> Result<(PathBuf, UpstreamHeaders), ProviderError> {
        let target = std::env::temp_dir().join(format!("radverkehr-{}.zip", Uuid::new_v4()));
        let headers = self
            .fetcher
            .get(&self.url, &target)
            .map_err(|message| ProviderError::Unreachable(format!("download failed: {message}")))?;
        self.store_state(KEY_DOWNLOADED_AT, &now.to_rfc3339())?;
        self.store_state(KEY_ARCHIVE_FILE, &target.to_string_lossy())?;
        self.emit(
            ProviderMessageSeverity::Info,
            format!("archive downloaded: {}", target.to_string_lossy()),
        );
        Ok((target, headers))
    }

    /// Extracts the ZIP into a fresh obscured temp directory.
    fn extract(&self, zip_path: &Path) -> Result<PathBuf, ProviderError> {
        let dest = std::env::temp_dir().join(format!("radverkehr-extracted-{}", Uuid::new_v4()));
        fs::create_dir_all(&dest).map_err(|e| {
            ProviderError::Storage(format!("cannot create extract dir {dest:?}: {e}"))
        })?;

        let file = File::open(zip_path)
            .map_err(|e| ProviderError::Storage(format!("cannot open zip {:?}: {e}", zip_path)))?;
        let mut archive = zip::ZipArchive::new(file)
            .map_err(|e| ProviderError::InvalidData(format!("invalid zip archive: {e}")))?;

        for i in 0..archive.len() {
            let mut entry = archive.by_index(i).map_err(|e| {
                ProviderError::InvalidData(format!("cannot read zip entry {i}: {e}"))
            })?;
            let Some(relative) = sanitize_zip_path(entry.name()) else {
                continue;
            };
            let target = dest.join(relative);
            if entry.is_dir() {
                fs::create_dir_all(&target).map_err(|e| {
                    ProviderError::Storage(format!("cannot create dir {target:?}: {e}"))
                })?;
            } else {
                if let Some(parent) = target.parent() {
                    fs::create_dir_all(parent).map_err(|e| {
                        ProviderError::Storage(format!("cannot create dir {parent:?}: {e}"))
                    })?;
                }
                let mut buffer = Vec::new();
                std::io::Read::read_to_end(&mut entry, &mut buffer)
                    .map_err(|e| ProviderError::InvalidData(format!("zip read error: {e}")))?;
                fs::write(&target, buffer)
                    .map_err(|e| ProviderError::Storage(format!("cannot write {target:?}: {e}")))?;
            }
        }
        self.emit(
            ProviderMessageSeverity::Info,
            format!("archive extracted: {}", dest.to_string_lossy()),
        );
        Ok(dest)
    }

    fn store_extracted(&self, dir: &Path, now: DateTime<Utc>) -> Result<(), ProviderError> {
        self.store_state(KEY_EXTRACTED_AT, &now.to_rfc3339())?;
        self.store_state(KEY_EXTRACTED_DIR, &dir.to_string_lossy())
    }

    fn store_refreshed(
        &self,
        zip: &Path,
        dir: &Path,
        now: DateTime<Utc>,
        headers: &UpstreamHeaders,
    ) -> Result<(), ProviderError> {
        self.store_extracted(dir, now)?;
        if let Some(etag) = &headers.etag {
            self.store_state(KEY_ETAG, etag)?;
        }
        if let Some(last_modified) = &headers.last_modified {
            self.store_state(KEY_LAST_MODIFIED, last_modified)?;
        }
        let _ = zip;
        Ok(())
    }

    /// Builds the in-memory index from the currently extracted archive.
    fn build_index(&self) -> Result<Arc<ArchiveIndex>, ProviderError> {
        let state = self.load_state()?;
        let extracted_dir = state
            .get(KEY_EXTRACTED_DIR)
            .map(PathBuf::from)
            .ok_or_else(|| {
                ProviderError::InvalidData("no extracted archive directory in state".to_string())
            })?;
        let root = extracted_dir.join(ARCHIVE_ROOT);

        let site_path = root.join(SITE_INDEX_FILE);
        let site_json = fs::read_to_string(&site_path).map_err(|e| {
            ProviderError::InvalidData(format!("cannot read site index {site_path:?}: {e}"))
        })?;
        let (stations, channels) = parse_site_index(&site_json)?;

        // Map each channel's external id -> the sorted monthly CSVs of its
        // station. All channels of a station share the station directory's
        // monthly files, so no CSV header reads are needed.
        let mut channel_csvs: HashMap<String, Vec<PathBuf>> = HashMap::new();
        for channel in &channels {
            let station_dir = root.join(&channel.counting_station_external_id);
            let mut csvs = Vec::new();
            let entries = match fs::read_dir(&station_dir) {
                Ok(entries) => entries,
                Err(_) => continue, // station folder missing: no files for it
            };
            for entry in entries {
                let Ok(entry) = entry else {
                    continue;
                };
                let csv_path = entry.path();
                if csv_path.extension().and_then(|e| e.to_str()) == Some("csv") {
                    csvs.push(csv_path);
                }
            }
            csvs.sort();
            channel_csvs.insert(channel.external_id.clone(), csvs);
        }

        Ok(Arc::new(ArchiveIndex {
            stations,
            channels,
            channel_csvs,
            extracted_dir,
        }))
    }

    // -- measurement serving -------------------------------------------------

    /// First measurement timestamp of the channel across all its monthly files.
    /// Used to anchor the first window when no page cursor (`from`) is given.
    fn earliest_timestamp(
        &self,
        csvs: &[PathBuf],
        channel_external_id: &str,
    ) -> Result<Option<DateTime<Utc>>, ProviderError> {
        for csv in csvs {
            let rows =
                parse_measurement_csv(csv, channel_external_id, self.messages_sink().as_deref())?;
            if let Some(first) = rows.into_iter().min_by_key(|record| record.timestamp) {
                return Ok(Some(first.timestamp));
            }
        }
        Ok(None)
    }

    /// Parses only the monthly files overlapping `(window_start, window_end]`,
    /// returning the in-window records (ascending) and whether data exists
    /// beyond `window_end` (a later monthly file, or later rows in the
    /// overlapping files).
    fn windowed_series(
        &self,
        channel_external_id: &str,
        csvs: &[PathBuf],
        window_start: DateTime<Utc>,
        window_end: DateTime<Utc>,
    ) -> Result<(Vec<MeasurementRecord>, bool), ProviderError> {
        let start_date = window_start.date_naive();
        let end_date = window_end.date_naive();
        let mut in_window = Vec::new();
        let mut data_beyond = false;

        for csv in csvs {
            let Some((file_start, file_end)) = csv_month_range(csv) else {
                continue;
            };
            if file_start > end_date {
                // Entire file lies after the window: more data exists time-wise.
                data_beyond = true;
                continue;
            }
            if file_end <= start_date {
                // Entire file lies before the window.
                continue;
            }
            let rows =
                parse_measurement_csv(csv, channel_external_id, self.messages_sink().as_deref())?;
            data_beyond |= rows.iter().any(|record| record.timestamp > window_end);
            in_window.extend(rows.into_iter().filter(|record| {
                record.timestamp > window_start && record.timestamp <= window_end
            }));
        }

        in_window.sort_by_key(|record| record.timestamp);
        Ok((in_window, data_beyond))
    }
}

impl DataProvider for MuensterGithubAdapter {
    fn check_health(&self) -> HealthStatus {
        let (host, port) = match parse_host_and_port(&self.url) {
            Some(host_port) => host_port,
            None => return HealthStatus::Down("invalid url in provider config".to_string()),
        };

        match TcpStream::connect((host, port)) {
            Ok(_) => HealthStatus::Up,
            Err(error) => HealthStatus::Down(format!("{error:?}")),
        }
    }

    fn get_all_counting_stations(&self) -> Result<Vec<CountingStationRecord>, ProviderError> {
        Ok(self.ensure_archive()?.stations.clone())
    }

    fn get_all_channels(&self) -> Result<Vec<ChannelRecord>, ProviderError> {
        Ok(self.ensure_archive()?.channels.clone())
    }

    fn get_measurements(&self, query: MeasurementQuery) -> Result<MeasurementBatch, ProviderError> {
        let index = self.ensure_archive()?;
        let channel_external_id = query
            .channel
            .external_datasource_id
            .as_ref()
            .map(|external| external.0.clone())
            .ok_or_else(|| ProviderError::InvalidData("channel has no external id".to_string()))?;

        let csvs = index
            .channel_csvs
            .get(&channel_external_id)
            .cloned()
            .unwrap_or_default();

        let timeframe = self.max_measurement_timeframe;
        let earliest = self.earliest_timestamp(&csvs, &channel_external_id)?;
        let Some(earliest) = earliest else {
            // The channel has no data anywhere: nothing to page.
            return Ok(MeasurementBatch {
                measurements: Vec::new(),
                last_measurement_datetime: query.from,
                batch_size_limit_reached: false,
                timeframe_limit_reached: false,
            });
        };

        // The window start is the page cursor (exclusive). Without a cursor,
        // start just before the earliest sample so the first sample is included.
        let window_start = query
            .from
            .unwrap_or_else(|| earliest - Duration::seconds(1));
        let window_end = match query.to {
            Some(to) => to,
            None => query.from.unwrap_or(earliest) + timeframe,
        };

        let (mut records, data_beyond) =
            self.windowed_series(&channel_external_id, &csvs, window_start, window_end)?;

        let batch_size_limit_reached = records.len() > query.max_batch_size;
        records.truncate(query.max_batch_size);

        // Advance past gaps: when the window holds no rows, report the window
        // end as the next cursor so the core can keep moving forward.
        let last_measurement_datetime = if records.is_empty() {
            Some(window_end)
        } else {
            records.last().map(|record| record.timestamp)
        };

        Ok(MeasurementBatch {
            measurements: records,
            last_measurement_datetime,
            batch_size_limit_reached,
            timeframe_limit_reached: query.to.is_none() && data_beyond,
        })
    }

    fn max_measurement_batch_size(&self) -> usize {
        self.max_measurement_batch_size
    }

    fn attach_persistent_state(&self, state: Arc<dyn PersistentStateAccess + Send + Sync>) {
        *self.state.lock().unwrap() = Some(state);
    }

    fn attach_provider_messages(&self, sink: Arc<dyn ProviderMessageSink + Send + Sync>) {
        *self.messages.lock().unwrap() = Some(sink);
    }
}

// ---------------------------------------------------------------------------
// Parsing helpers.
// ---------------------------------------------------------------------------

/// Raw shape of `site_min.json`.
#[derive(serde::Deserialize)]
struct RawSite {
    name: String,
    directory: String,
    #[allow(dead_code)]
    start: i64,
    channels: Vec<(i64, String)>,
}

/// Parses the site index into station and channel records, skipping the station
/// aggregate entry (`id == directory`).
fn parse_site_index(
    json: &str,
) -> Result<(Vec<CountingStationRecord>, Vec<ChannelRecord>), ProviderError> {
    let sites: Vec<RawSite> = serde_json::from_str(json)
        .map_err(|e| ProviderError::InvalidData(format!("invalid site_min.json: {e}")))?;

    let mut stations = Vec::with_capacity(sites.len());
    let mut channels = Vec::new();
    for site in sites {
        let station_external_id = site.directory.clone();
        for (id, name) in site.channels {
            let id = id.to_string();
            if id == station_external_id {
                // Station aggregate column: redundant with the sum of channels.
                continue;
            }
            channels.push(ChannelRecord {
                external_id: id,
                counting_station_external_id: station_external_id.clone(),
                name,
                description: String::new(),
            });
        }
        stations.push(CountingStationRecord {
            external_id: station_external_id,
            name: site.name,
            description: String::new(),
        });
    }
    Ok((stations, channels))
}

/// Parses the `YYYY-MM` filename of a monthly CSV into its `[start, end)`
/// month range as UTC dates. `None` when the filename is not a monthly CSV.
fn csv_month_range(path: &Path) -> Option<(NaiveDate, NaiveDate)> {
    let file_name = path.file_name()?.to_str()?;
    let stem = file_name.strip_suffix(".csv")?;
    let mut parts = stem.split('-');
    let year: i32 = parts.next()?.parse().ok()?;
    let month: u32 = parts.next()?.parse().ok()?;
    if !(1..=12).contains(&month) {
        return None;
    }
    let start = NaiveDate::from_ymd_opt(year, month, 1)?;
    let end = if month == 12 {
        NaiveDate::from_ymd_opt(year + 1, 1, 1)?
    } else {
        NaiveDate::from_ymd_opt(year, month + 1, 1)?
    };
    Some((start, end))
}

/// Parses the measurements of one channel from a monthly CSV.
///
/// A channel that is simply absent from a file is a *known, non-fatal data
/// quirk*: a `WARNING` is emitted (when a sink is available) and an empty batch
/// is returned so the import continues. Genuine IO/parse failures (unreadable
/// file, invalid CSV header) still return `ProviderError` and fail the job.
fn parse_measurement_csv(
    path: &Path,
    channel_external_id: &str,
    messages: Option<&(dyn ProviderMessageSink + Send + Sync)>,
) -> Result<Vec<MeasurementRecord>, ProviderError> {
    let file = File::open(path)
        .map_err(|e| ProviderError::InvalidData(format!("cannot open {path:?}: {e}")))?;
    let mut reader = csv::ReaderBuilder::new()
        .has_headers(true)
        .from_reader(file);

    let headers = reader
        .headers()
        .map_err(|e| ProviderError::InvalidData(format!("invalid CSV header in {path:?}: {e}")))?
        .clone();

    // Locate the data column for the channel; its header is "<id> (<name>)".
    let wanted = format!("{channel_external_id} ");
    let column = match headers
        .iter()
        .position(|header| header.starts_with(&wanted))
    {
        Some(column) => column,
        None => {
            // Known quirk: warn and skip this file instead of aborting the
            // whole import with a major job failure.
            if let Some(messages) = messages {
                let message = format!("channel {channel_external_id} has no column in {path:?}");
                let _ =
                    messages.provider_event_occurred(ProviderMessageSeverity::Warning, &message);
            }
            return Ok(Vec::new());
        }
    };

    let mut records = Vec::new();
    for result in reader.records() {
        let record = result
            .map_err(|e| ProviderError::InvalidData(format!("invalid CSV row in {path:?}: {e}")))?;
        let Some(timestamp_text) = record.get(0) else {
            continue;
        };
        let Some(naive) = NaiveDateTime::parse_from_str(timestamp_text.trim(), "%Y-%m-%d %H:%M")
            .ok()
            .or_else(|| {
                NaiveDateTime::parse_from_str(timestamp_text.trim(), "%Y-%m-%d %H:%M:%S").ok()
            })
        else {
            continue;
        };
        let Some(timestamp) = berlin_to_utc(naive) else {
            continue;
        };
        let value_text = record.get(column).unwrap_or("");
        let value_text = value_text.trim();
        if value_text.is_empty() {
            continue;
        }
        let Ok(value) = value_text.parse::<i64>() else {
            continue;
        };
        records.push(MeasurementRecord { value, timestamp });
    }
    Ok(records)
}

/// Converts a naive local timestamp (Europe/Berlin, DST-aware) to UTC.
fn berlin_to_utc(naive: NaiveDateTime) -> Option<DateTime<Utc>> {
    TIMEZONE
        .from_local_datetime(&naive)
        .single()
        .or_else(|| TIMEZONE.from_local_datetime(&naive).earliest())
        .map(|local| local.with_timezone(&Utc))
}

fn parse_rfc3339(value: &str) -> Option<DateTime<Utc>> {
    DateTime::parse_from_rfc3339(value)
        .ok()
        .map(|dt| dt.with_timezone(&Utc))
}

/// Turns a zip entry name into a safe relative path, rejecting any that would
/// escape the extraction directory.
fn sanitize_zip_path(name: &str) -> Option<PathBuf> {
    let mut out = PathBuf::new();
    for component in Path::new(name).components() {
        match component {
            Component::Normal(part) => out.push(part),
            Component::CurDir => {}
            _ => return None,
        }
    }
    if out.as_os_str().is_empty() {
        None
    } else {
        Some(out)
    }
}

/// Naive host/port extraction for health checks (no extra dependency).
fn parse_host_and_port(url: &str) -> Option<(String, u16)> {
    let rest = url.split_once("://")?.1;
    let host_port = rest.split(['/', '?', '#']).next()?;
    if let Some((host, port)) = host_port.rsplit_once(':') {
        return Some((host.to_string(), port.parse().ok()?));
    }
    let port = if url.starts_with("https://") { 443 } else { 80 };
    Some((host_port.to_string(), port))
}

#[cfg(test)]
mod tests {
    use std::collections::HashMap;
    use std::io::Write;
    use std::sync::{Arc, Mutex};

    use super::*;
    use crate::core::domain::channels::channel::Channel;
    use crate::core::domain::configuration::configuration::value_objects::{
        DataProviderConfiguration, DataSourceConfiguration,
    };
    use crate::core::domain::configuration::error::ConfigError;
    use crate::core::domain::data_source::provider::{PersistentStateAccess, ProviderMessageSink};
    use crate::core::domain::data_source::provider_message::ProviderMessageSeverity;

    fn data_source(vars: HashMap<String, String>) -> DataSourceConfiguration {
        let provider = DataProviderConfiguration::new(
            MuensterGithubAdapter::provider_type().to_string(),
            vars,
        )
        .unwrap();
        DataSourceConfiguration::new("Münster".to_string(), provider).unwrap()
    }

    // -- config parsing ------------------------------------------------------

    #[test]
    fn parses_host_and_port() {
        assert_eq!(
            parse_host_and_port(
                "https://github.com/od-ms/radverkehr-zaehlstellen/archive/refs/heads/main.zip"
            ),
            Some(("github.com".to_string(), 443))
        );
        assert_eq!(
            parse_host_and_port("http://example.com:8080/path"),
            Some(("example.com".to_string(), 8080))
        );
    }

    #[test]
    fn rejects_missing_url_var() {
        let config = data_source(HashMap::new());
        assert!(matches!(
            MuensterGithubAdapter::new(&config),
            Err(ConfigError::InvalidFormat(_))
        ));
    }

    #[test]
    fn rejects_invalid_batch_size_var() {
        let mut vars = HashMap::new();
        vars.insert("url".to_string(), "https://github.com".to_string());
        vars.insert(
            "max_measurement_batch_size".to_string(),
            "not-a-number".to_string(),
        );
        let config = data_source(vars);
        assert!(matches!(
            MuensterGithubAdapter::new(&config),
            Err(ConfigError::InvalidFormat(_))
        ));
    }

    #[test]
    fn defaults_batch_size_when_unset() {
        let mut vars = HashMap::new();
        vars.insert("url".to_string(), "https://github.com".to_string());
        let config = data_source(vars);
        let adapter = MuensterGithubAdapter::new(&config).unwrap();
        assert_eq!(adapter.max_measurement_batch_size(), 500);
    }

    #[test]
    fn reports_down_for_unreachable_url() {
        let mut vars = HashMap::new();
        vars.insert(
            "url".to_string(),
            "http://127.0.0.1:1/archive.zip".to_string(),
        );
        let config = data_source(vars);
        let adapter = MuensterGithubAdapter::new(&config).unwrap();
        assert!(matches!(adapter.check_health(), HealthStatus::Down(_)));
    }

    #[test]
    fn defaults_cache_duration_when_unset() {
        let mut vars = HashMap::new();
        vars.insert("url".to_string(), "https://github.com".to_string());
        let config = data_source(vars);
        let adapter = MuensterGithubAdapter::new(&config).unwrap();
        assert_eq!(adapter.cache_duration(), 300);
    }

    #[test]
    fn reads_cache_duration_var() {
        let mut vars = HashMap::new();
        vars.insert("url".to_string(), "https://github.com".to_string());
        vars.insert("cache_duration".to_string(), "120".to_string());
        let config = data_source(vars);
        let adapter = MuensterGithubAdapter::new(&config).unwrap();
        assert_eq!(adapter.cache_duration(), 120);
    }

    #[test]
    fn rejects_invalid_cache_duration_var() {
        let mut vars = HashMap::new();
        vars.insert("url".to_string(), "https://github.com".to_string());
        vars.insert("cache_duration".to_string(), "not-a-number".to_string());
        let config = data_source(vars);
        assert!(matches!(
            MuensterGithubAdapter::new(&config),
            Err(ConfigError::InvalidFormat(_))
        ));
    }

    #[test]
    fn defaults_measurement_timeframe_when_unset() {
        let mut vars = HashMap::new();
        vars.insert("url".to_string(), "https://github.com".to_string());
        let config = data_source(vars);
        let adapter = MuensterGithubAdapter::new(&config).unwrap();
        assert_eq!(adapter.max_measurement_timeframe_hours(), 168);
    }

    #[test]
    fn reads_measurement_timeframe_var() {
        let mut vars = HashMap::new();
        vars.insert("url".to_string(), "https://github.com".to_string());
        vars.insert(
            "max_measurement_timeframe_hours".to_string(),
            "24".to_string(),
        );
        let config = data_source(vars);
        let adapter = MuensterGithubAdapter::new(&config).unwrap();
        assert_eq!(adapter.max_measurement_timeframe_hours(), 24);
    }

    #[test]
    fn rejects_invalid_measurement_timeframe_var() {
        let mut vars = HashMap::new();
        vars.insert("url".to_string(), "https://github.com".to_string());
        vars.insert(
            "max_measurement_timeframe_hours".to_string(),
            "not-a-number".to_string(),
        );
        let config = data_source(vars);
        assert!(matches!(
            MuensterGithubAdapter::new(&config),
            Err(ConfigError::InvalidFormat(_))
        ));
    }

    /// In-memory persistent-state access used to exercise the attach hook.
    #[derive(Default)]
    struct InMemoryAccess {
        map: Mutex<HashMap<String, String>>,
    }

    impl PersistentStateAccess for InMemoryAccess {
        fn load(&self) -> Result<HashMap<String, String>, ProviderError> {
            Ok(self.map.lock().unwrap().clone())
        }

        fn store(&self, key: &str, value: &str) -> Result<(), ProviderError> {
            self.map
                .lock()
                .unwrap()
                .insert(key.to_string(), value.to_string());
            Ok(())
        }

        fn delete(&self, key: &str) -> Result<(), ProviderError> {
            self.map.lock().unwrap().remove(key);
            Ok(())
        }

        fn clear(&self) -> Result<(), ProviderError> {
            self.map.lock().unwrap().clear();
            Ok(())
        }
    }

    #[test]
    fn attach_persistent_state_stores_the_handle() {
        let mut vars = HashMap::new();
        vars.insert("url".to_string(), "https://github.com".to_string());
        let config = data_source(vars);
        let adapter = MuensterGithubAdapter::new(&config).unwrap();

        assert!(adapter.attached_state().is_none());
        adapter.attach_persistent_state(Arc::new(InMemoryAccess::default()));
        assert!(adapter.attached_state().is_some());
    }

    // -- parsers -------------------------------------------------------------

    #[test]
    fn parse_site_index_skips_the_aggregate_entry() {
        let json = r#"[
            {
                "name": "Promenade (nördl. Salzstraße)",
                "directory": "100031297",
                "start": 2023,
                "channels": [
                    [100031297, "Promenade (nördl. Salzstraße)"],
                    [101031297, "Promenade Radfahrer FR Mauritztor"],
                    [102031297, "Promenade Radfahrer FR Salzstraße"]
                ]
            }
        ]"#;

        let (stations, channels) = parse_site_index(json).unwrap();

        assert_eq!(stations.len(), 1);
        assert_eq!(stations[0].external_id, "100031297");
        assert_eq!(stations[0].name, "Promenade (nördl. Salzstraße)");

        assert_eq!(channels.len(), 2, "the aggregate entry must be skipped");
        assert_eq!(channels[0].external_id, "101031297");
        assert_eq!(channels[0].counting_station_external_id, "100031297");
        assert_eq!(channels[1].external_id, "102031297");
    }

    #[test]
    fn berlin_to_utc_handles_winter_time() {
        let naive = NaiveDateTime::parse_from_str("2024-01-15 12:00", "%Y-%m-%d %H:%M").unwrap();
        assert_eq!(
            berlin_to_utc(naive).unwrap().to_rfc3339(),
            "2024-01-15T11:00:00+00:00"
        );
    }

    #[test]
    fn berlin_to_utc_handles_summer_time() {
        let naive = NaiveDateTime::parse_from_str("2024-07-15 12:00", "%Y-%m-%d %H:%M").unwrap();
        assert_eq!(
            berlin_to_utc(naive).unwrap().to_rfc3339(),
            "2024-07-15T10:00:00+00:00"
        );
    }

    #[test]
    fn parse_measurement_csv_filters_by_channel_and_skips_status() {
        let dir = std::env::temp_dir().join(format!("csv-test-{}", Uuid::new_v4()));
        fs::create_dir_all(&dir).unwrap();
        let path = dir.join("2023-01.csv");
        let csv = concat!(
            "Datetime,100031297 (Promenade),101031297 (Radfahrer FR),102031297 (Radfahrer FR),100031297-status,101031297-status\n",
            "2023-01-01 00:00,3,5,,0,1\n",
            "2023-01-01 00:15,22,7,4,0,0\n",
            "not-a-timestamp,1,1,1,0,0\n",
        );
        fs::write(&path, csv).unwrap();

        let records = parse_measurement_csv(&path, "102031297", None).unwrap();
        assert_eq!(records.len(), 1, "empty and invalid rows are skipped");
        assert_eq!(records[0].value, 4);
        assert_eq!(
            records[0].timestamp.to_rfc3339(),
            "2022-12-31T23:15:00+00:00"
        );

        fs::remove_dir_all(&dir).unwrap();
    }

    #[test]
    fn csv_month_range_parses_the_filename_bounds() {
        assert_eq!(
            csv_month_range(Path::new("2023-01.csv")),
            Some((
                NaiveDate::from_ymd_opt(2023, 1, 1).unwrap(),
                NaiveDate::from_ymd_opt(2023, 2, 1).unwrap()
            ))
        );
        assert_eq!(
            csv_month_range(Path::new("2023-12.csv")),
            Some((
                NaiveDate::from_ymd_opt(2023, 12, 1).unwrap(),
                NaiveDate::from_ymd_opt(2024, 1, 1).unwrap()
            ))
        );
        assert_eq!(csv_month_range(Path::new("readme.md")), None);
        assert_eq!(csv_month_range(Path::new("2023-13.csv")), None);
    }

    // -- archive cache + data serving ----------------------------------------

    /// Fake fetcher: serves a prebuilt zip and records how often it is hit.
    struct FakeFetcher {
        zip: Vec<u8>,
        etag: Option<String>,
        get_calls: Mutex<usize>,
    }

    impl ArchiveFetcher for FakeFetcher {
        fn head(&self, _url: &str) -> Option<UpstreamHeaders> {
            Some(UpstreamHeaders {
                etag: self.etag.clone(),
                last_modified: None,
            })
        }

        fn get(&self, _url: &str, target: &Path) -> Result<UpstreamHeaders, String> {
            *self.get_calls.lock().unwrap() += 1;
            fs::write(target, &self.zip).map_err(|e| e.to_string())?;
            Ok(UpstreamHeaders {
                etag: self.etag.clone(),
                last_modified: None,
            })
        }
    }

    fn fixture_site_json() -> &'static str {
        r#"[
            {
                "name": "Promenade (nördl. Salzstraße)",
                "directory": "100031297",
                "start": 2023,
                "channels": [
                    [100031297, "Promenade (nördl. Salzstraße)"],
                    [101031297, "Promenade Radfahrer FR Mauritztor"],
                    [102031297, "Promenade Radfahrer FR Salzstraße"]
                ]
            }
        ]"#
    }

    fn fixture_csv() -> &'static str {
        concat!(
            "Datetime,100031297 (Promenade),101031297 (Radfahrer),102031297 (Radfahrer),100031297-status,101031297-status,102031297-status\n",
            "2023-01-01 00:00,3,5,1,0,0,0\n",
            "2023-01-01 00:15,22,7,4,0,0,0\n",
            "2023-01-01 00:30,37,9,8,0,0,0\n",
        )
    }

    fn build_fixture_zip() -> Vec<u8> {
        let mut buffer = std::io::Cursor::new(Vec::new());
        let options = zip::write::SimpleFileOptions::default();
        {
            let mut writer = zip::ZipWriter::new(&mut buffer);
            writer
                .start_file(format!("{ARCHIVE_ROOT}/{SITE_INDEX_FILE}"), options)
                .unwrap();
            writer.write_all(fixture_site_json().as_bytes()).unwrap();
            writer
                .start_file(format!("{ARCHIVE_ROOT}/100031297/2023-01.csv"), options)
                .unwrap();
            writer.write_all(fixture_csv().as_bytes()).unwrap();
            writer.finish().unwrap();
        }
        buffer.into_inner()
    }

    /// Writes a fixture archive to `dir` as a real extracted directory.
    fn write_extracted_fixture(dir: &Path) {
        let root = dir.join(ARCHIVE_ROOT);
        fs::create_dir_all(root.join("100031297")).unwrap();
        fs::write(root.join(SITE_INDEX_FILE), fixture_site_json()).unwrap();
        fs::write(root.join("100031297/2023-01.csv"), fixture_csv()).unwrap();
    }

    fn adapter_with(
        config: DataSourceConfiguration,
        fetcher: Arc<dyn ArchiveFetcher>,
    ) -> MuensterGithubAdapter {
        MuensterGithubAdapter::with_fetcher(&config, fetcher).unwrap()
    }

    #[test]
    fn serves_stations_channels_and_measurements_from_a_fresh_archive() {
        // Build a real extracted fixture on disk and point state at it.
        let fixture = std::env::temp_dir().join(format!("fixture-{}", Uuid::new_v4()));
        write_extracted_fixture(&fixture);

        let state = Arc::new(InMemoryAccess::default());
        state
            .store(KEY_EXTRACTED_DIR, &fixture.to_string_lossy())
            .unwrap();
        state
            .store(KEY_EXTRACTED_AT, &Utc::now().to_rfc3339())
            .unwrap();

        let mut vars = HashMap::new();
        vars.insert("url".to_string(), "https://github.com".to_string());
        vars.insert("cache_duration".to_string(), "3600".to_string());
        let config = data_source(vars);
        let fetcher = Arc::new(FakeFetcher {
            zip: build_fixture_zip(),
            etag: None,
            get_calls: Mutex::new(0),
        });
        let adapter = adapter_with(config, fetcher.clone());
        adapter.attach_persistent_state(state);

        let stations = adapter.get_all_counting_stations().unwrap();
        assert_eq!(stations.len(), 1);
        assert_eq!(stations[0].external_id, "100031297");

        let channels = adapter.get_all_channels().unwrap();
        assert_eq!(channels.len(), 2, "aggregate channel is skipped");

        // Query all measurements for the second channel, paged in twos.
        let channel = Channel {
            id: crate::core::domain::channels::channel::value_objects::Id(Uuid::new_v4()),
            counting_station_id:
                crate::core::domain::channels::channel::value_objects::CountingStationId(
                    Uuid::new_v4(),
                ),
            name: crate::core::domain::channels::channel::value_objects::Name(
                "Radfahrer".to_string(),
            ),
            description: crate::core::domain::channels::channel::value_objects::Description(
                String::new(),
            ),
            external_datasource_id: Some(
                crate::core::domain::channels::channel::value_objects::ExternalDatasourceId(
                    "102031297".to_string(),
                ),
            ),
        };
        let first = adapter
            .get_measurements(MeasurementQuery::for_channel(channel.clone(), 2))
            .unwrap();
        assert_eq!(first.measurements.len(), 2);
        assert_eq!(first.measurements[0].value, 1);
        assert_eq!(first.measurements[1].value, 4);
        assert!(first.batch_size_limit_reached);
        let resume = first.last_measurement_datetime.unwrap();
        assert_eq!(resume.to_rfc3339(), "2022-12-31T23:15:00+00:00");

        let second = adapter
            .get_measurements(MeasurementQuery::for_channel(channel.clone(), 2).with_start(resume))
            .unwrap();
        assert_eq!(second.measurements.len(), 1, "from is exclusive");
        assert!(!second.batch_size_limit_reached);
        assert_eq!(second.measurements[0].value, 8);
        assert_eq!(
            second.measurements[0].timestamp.to_rfc3339(),
            "2022-12-31T23:30:00+00:00"
        );

        // Tier 1: the fresh extracted dir is reused; no download happened.
        assert_eq!(*fetcher.get_calls.lock().unwrap(), 0);

        fs::remove_dir_all(&fixture).unwrap();
    }

    #[test]
    fn get_measurements_windows_by_timeframe_and_advances_past_gaps() {
        let fixture = std::env::temp_dir().join(format!("fixture-{}", Uuid::new_v4()));
        let root = fixture.join(ARCHIVE_ROOT);
        fs::create_dir_all(root.join("100031297")).unwrap();
        fs::write(root.join(SITE_INDEX_FILE), fixture_site_json()).unwrap();
        fs::write(
            root.join("100031297/2023-01.csv"),
            concat!(
                "Datetime,100031297 (Promenade),101031297 (Radfahrer),102031297 (Radfahrer),100031297-status,101031297-status,102031297-status\n",
                "2023-01-01 00:00,3,5,1,0,0,0\n",
                "2023-01-02 00:00,3,5,2,0,0,0\n",
                "2023-01-05 00:00,3,5,9,0,0,0\n",
            ),
        )
        .unwrap();

        let state = Arc::new(InMemoryAccess::default());
        state
            .store(KEY_EXTRACTED_DIR, &fixture.to_string_lossy())
            .unwrap();
        state
            .store(KEY_EXTRACTED_AT, &Utc::now().to_rfc3339())
            .unwrap();

        let mut vars = HashMap::new();
        vars.insert("url".to_string(), "https://github.com".to_string());
        vars.insert(
            "max_measurement_timeframe_hours".to_string(),
            "48".to_string(),
        );
        let config = data_source(vars);
        let fetcher = Arc::new(FakeFetcher {
            zip: Vec::new(),
            etag: None,
            get_calls: Mutex::new(0),
        });
        let adapter = adapter_with(config, fetcher);
        adapter.attach_persistent_state(state);

        let channel = Channel {
            id: crate::core::domain::channels::channel::value_objects::Id(Uuid::new_v4()),
            counting_station_id:
                crate::core::domain::channels::channel::value_objects::CountingStationId(
                    Uuid::new_v4(),
                ),
            name: crate::core::domain::channels::channel::value_objects::Name(
                "Radfahrer".to_string(),
            ),
            description: crate::core::domain::channels::channel::value_objects::Description(
                String::new(),
            ),
            external_datasource_id: Some(
                crate::core::domain::channels::channel::value_objects::ExternalDatasourceId(
                    "102031297".to_string(),
                ),
            ),
        };

        // Page 1: first two days (48h window); more data exists beyond it.
        let first = adapter
            .get_measurements(MeasurementQuery::for_channel(channel.clone(), 500))
            .unwrap();
        assert_eq!(first.measurements.len(), 2);
        assert!(!first.batch_size_limit_reached);
        assert!(first.timeframe_limit_reached);
        assert_eq!(
            first.last_measurement_datetime.unwrap().to_rfc3339(),
            "2023-01-01T23:00:00+00:00"
        );

        // Page 2: the ~3-day gap yields an empty window that advances to its end.
        let gap = first.last_measurement_datetime.unwrap();
        let second = adapter
            .get_measurements(MeasurementQuery::for_channel(channel.clone(), 500).with_start(gap))
            .unwrap();
        assert!(second.measurements.is_empty());
        assert!(second.timeframe_limit_reached);
        assert_eq!(
            second.last_measurement_datetime.unwrap().to_rfc3339(),
            "2023-01-03T23:00:00+00:00"
        );

        // Page 3: the final day is imported and the channel is complete.
        let resume = second.last_measurement_datetime.unwrap();
        let third = adapter
            .get_measurements(
                MeasurementQuery::for_channel(channel.clone(), 500).with_start(resume),
            )
            .unwrap();
        assert_eq!(third.measurements.len(), 1);
        assert_eq!(third.measurements[0].value, 9);
        assert!(!third.batch_size_limit_reached);
        assert!(!third.timeframe_limit_reached);
        assert_eq!(
            third.last_measurement_datetime.unwrap().to_rfc3339(),
            "2023-01-04T23:00:00+00:00"
        );

        fs::remove_dir_all(&fixture).unwrap();
    }

    #[test]
    fn windowed_series_only_reads_overlapping_month_files() {
        let dir = std::env::temp_dir().join(format!("window-{}", Uuid::new_v4()));
        fs::create_dir_all(dir.join("100031297")).unwrap();
        // Files before and after the window omit the channel column; if they were
        // parsed, `parse_measurement_csv` would error. Only the overlapping
        // month may be read.
        fs::write(
            dir.join("100031297/2022-12.csv"),
            "Datetime,100031297 (Promenade)\n2022-12-15 00:00,1\n",
        )
        .unwrap();
        fs::write(
            dir.join("100031297/2023-01.csv"),
            concat!(
                "Datetime,100031297 (Promenade),101031297 (Radfahrer),102031297 (Radfahrer),100031297-status,101031297-status,102031297-status\n",
                "2023-01-15 00:00,3,5,1,0,0,0\n",
                "2023-01-15 00:15,3,5,4,0,0,0\n",
            ),
        )
        .unwrap();
        fs::write(
            dir.join("100031297/2023-03.csv"),
            "Datetime,100031297 (Promenade)\n2023-03-15 00:00,1\n",
        )
        .unwrap();

        let csvs = vec![
            dir.join("100031297/2022-12.csv"),
            dir.join("100031297/2023-01.csv"),
            dir.join("100031297/2023-03.csv"),
        ];

        let mut vars = HashMap::new();
        vars.insert("url".to_string(), "https://github.com".to_string());
        let config = data_source(vars);
        let fetcher = Arc::new(FakeFetcher {
            zip: Vec::new(),
            etag: None,
            get_calls: Mutex::new(0),
        });
        let adapter = adapter_with(config, fetcher);

        let window_start = DateTime::parse_from_rfc3339("2023-01-14T00:00:00Z")
            .unwrap()
            .with_timezone(&Utc);
        let window_end = DateTime::parse_from_rfc3339("2023-01-16T00:00:00Z")
            .unwrap()
            .with_timezone(&Utc);
        let (records, data_beyond) = adapter
            .windowed_series("102031297", &csvs, window_start, window_end)
            .unwrap();

        assert_eq!(records.len(), 2, "only the overlapping month is read");
        assert_eq!(records[0].value, 1);
        assert_eq!(records[1].value, 4);
        assert!(data_beyond, "the later March file signals more data");

        fs::remove_dir_all(&dir).unwrap();
    }

    #[test]
    fn reuses_a_fresh_zip_when_the_folder_is_missing() {
        // Seed state with a fresh ZIP on disk but no extracted folder.
        let zip_path = std::env::temp_dir().join(format!("archive-{}.zip", Uuid::new_v4()));
        fs::write(&zip_path, build_fixture_zip()).unwrap();

        let state = Arc::new(InMemoryAccess::default());
        state
            .store(KEY_ARCHIVE_FILE, &zip_path.to_string_lossy())
            .unwrap();
        state
            .store(KEY_DOWNLOADED_AT, &Utc::now().to_rfc3339())
            .unwrap();

        let mut vars = HashMap::new();
        vars.insert("url".to_string(), "https://github.com".to_string());
        vars.insert("cache_duration".to_string(), "3600".to_string());
        let config = data_source(vars);
        let fetcher = Arc::new(FakeFetcher {
            zip: build_fixture_zip(),
            etag: None,
            get_calls: Mutex::new(0),
        });
        let adapter = adapter_with(config, fetcher.clone());
        adapter.attach_persistent_state(state);

        // Tier 2 re-extracts from the ZIP without downloading.
        let stations = adapter.get_all_counting_stations().unwrap();
        assert_eq!(stations.len(), 1);
        assert_eq!(*fetcher.get_calls.lock().unwrap(), 0);

        fs::remove_file(&zip_path).unwrap();
    }

    #[test]
    fn downloads_when_the_cache_is_stale() {
        let stale = Utc::now() - chrono::Duration::hours(2);
        let state = Arc::new(InMemoryAccess::default());
        state.store(KEY_DOWNLOADED_AT, &stale.to_rfc3339()).unwrap();

        let mut vars = HashMap::new();
        vars.insert("url".to_string(), "https://github.com".to_string());
        vars.insert("cache_duration".to_string(), "3600".to_string());
        let config = data_source(vars);
        let fetcher = Arc::new(FakeFetcher {
            zip: build_fixture_zip(),
            etag: None,
            get_calls: Mutex::new(0),
        });
        let adapter = adapter_with(config, fetcher.clone());
        adapter.attach_persistent_state(state);

        let stations = adapter.get_all_counting_stations().unwrap();
        assert_eq!(stations.len(), 1);
        assert_eq!(
            *fetcher.get_calls.lock().unwrap(),
            1,
            "stale cache triggers a download"
        );
    }

    // -- provider messages ----------------------------------------------------

    /// In-memory provider-message sink recording every emitted event.
    #[derive(Default)]
    struct RecordingSink {
        events: Mutex<Vec<(ProviderMessageSeverity, String)>>,
    }

    impl ProviderMessageSink for RecordingSink {
        fn provider_event_occurred(
            &self,
            severity: ProviderMessageSeverity,
            message: &str,
        ) -> Result<(), ProviderError> {
            self.events
                .lock()
                .unwrap()
                .push((severity, message.to_string()));
            Ok(())
        }
    }

    #[test]
    fn missing_channel_column_emits_warning_and_returns_empty_batch() {
        let dir = std::env::temp_dir().join(format!("csv-warn-{}", Uuid::new_v4()));
        fs::create_dir_all(&dir).unwrap();
        let path = dir.join("2023-01.csv");
        // Only the station aggregate column exists; the queried channel is absent.
        fs::write(
            &path,
            "Datetime,100031297 (Promenade)\n2023-01-01 00:00,3\n",
        )
        .unwrap();

        let sink = RecordingSink::default();
        let records = parse_measurement_csv(&path, "102031297", Some(&sink)).unwrap();

        assert!(
            records.is_empty(),
            "a missing column is a known quirk and must not fail the parse"
        );
        let events = sink.events.lock().unwrap();
        assert_eq!(events.len(), 1);
        assert_eq!(events[0].0, ProviderMessageSeverity::Warning);
        assert!(events[0].1.contains("102031297"));
        assert!(events[0].1.contains("2023-01.csv"));

        fs::remove_dir_all(&dir).unwrap();
    }

    #[test]
    fn genuine_io_and_parse_errors_still_fail() {
        // Unreadable file -> InvalidData (job fails as before).
        let missing = std::env::temp_dir().join(format!("missing-{}.csv", Uuid::new_v4()));
        assert!(matches!(
            parse_measurement_csv(&missing, "1", None),
            Err(ProviderError::InvalidData(_))
        ));

        // Invalid (non-UTF-8) CSV header bytes -> InvalidData (job fails as
        // before); the csv reader cannot decode the header record.
        let dir = std::env::temp_dir().join(format!("csv-bad-{}", Uuid::new_v4()));
        fs::create_dir_all(&dir).unwrap();
        let path = dir.join("2023-01.csv");
        fs::write(&path, [0xFF, 0xFE, 0x00, 0x01]).unwrap();
        assert!(matches!(
            parse_measurement_csv(&path, "1", None),
            Err(ProviderError::InvalidData(_))
        ));

        fs::remove_dir_all(&dir).unwrap();
    }

    #[test]
    fn attach_provider_messages_stores_the_sink() {
        let mut vars = HashMap::new();
        vars.insert("url".to_string(), "https://github.com".to_string());
        let config = data_source(vars);
        let adapter = MuensterGithubAdapter::new(&config).unwrap();

        let sink = Arc::new(RecordingSink::default());
        adapter.attach_provider_messages(sink.clone());
        adapter.emit(ProviderMessageSeverity::Info, "archive downloaded");

        assert_eq!(sink.events.lock().unwrap().len(), 1);
    }

    #[test]
    fn emits_one_line_lifecycle_messages_on_cache_refresh() {
        let stale = Utc::now() - chrono::Duration::hours(2);
        let state = Arc::new(InMemoryAccess::default());
        state.store(KEY_DOWNLOADED_AT, &stale.to_rfc3339()).unwrap();

        let mut vars = HashMap::new();
        vars.insert("url".to_string(), "https://github.com".to_string());
        vars.insert("cache_duration".to_string(), "3600".to_string());
        let config = data_source(vars);
        let fetcher = Arc::new(FakeFetcher {
            zip: build_fixture_zip(),
            etag: None,
            get_calls: Mutex::new(0),
        });
        let adapter = adapter_with(config, fetcher);
        adapter.attach_persistent_state(state);

        let sink = Arc::new(RecordingSink::default());
        adapter.attach_provider_messages(sink.clone());

        // Stale cache forces tier 3: download + extract.
        adapter.get_all_counting_stations().unwrap();

        let events = sink.events.lock().unwrap();
        assert!(
            events.iter().any(|(severity, message)| {
                *severity == ProviderMessageSeverity::Info
                    && message.starts_with("archive downloaded:")
            }),
            "expected an INFO 'archive downloaded' one-liner, got: {:?}",
            *events
        );
        assert!(
            events.iter().any(|(severity, message)| {
                *severity == ProviderMessageSeverity::Info
                    && message.starts_with("archive extracted:")
            }),
            "expected an INFO 'archive extracted' one-liner, got: {:?}",
            *events
        );
    }
}
