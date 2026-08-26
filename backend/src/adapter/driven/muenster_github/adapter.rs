//! The Münster GitHub [`DataProvider`] adapter: configuration, the four-tier
//! archive cache lifecycle, and measurement serving.
//!
//! The HTTP fetch, archive index and parsing logic live in the sibling modules
//! [`fetcher`](super::fetcher), [`archive`](super::archive) and
//! [`parsing`](super::parsing), keeping each concern focused.

use std::collections::HashMap;
use std::fs::{self, File};
use std::net::TcpStream;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};

use chrono::{DateTime, Duration, Utc};
use uuid::Uuid;

use crate::core::domain::configuration::configuration::value_objects::DataSourceConfiguration;
use crate::core::domain::configuration::error::ConfigError;
use crate::core::domain::data_source::provider_message::ProviderMessageSeverity;
use crate::core::domain::data_source::provider_port::{
    DataProvider, MeasurementBatch, MeasurementQuery, PersistentStateAccess, ProviderError,
    ProviderMessageSink,
};
use crate::core::domain::health::HealthStatus;

use super::archive::{ARCHIVE_ROOT, ArchiveIndex, SITE_INDEX_FILE, sanitize_zip_path};
use super::fetcher::{ArchiveFetcher, HttpFetcher, UpstreamHeaders};
use super::parsing::{
    csv_month_range, parse_host_and_port, parse_measurement_csv, parse_rfc3339, parse_site_index,
};
use super::station_metadata;

pub(crate) const PROVIDER_TYPE: &str = "münster_opendata_github_provider";
pub(crate) const DEFAULT_MAX_MEASUREMENT_BATCH_SIZE: usize = 500;
/// Default import time window per provider call: 7 days, in hours.
pub(crate) const DEFAULT_MAX_MEASUREMENT_TIMEFRAME_HOURS: u64 = 168;
pub(crate) const DEFAULT_CACHE_DURATION_SECS: u64 = 300;

/// Persistent-state keys owned by this provider.
pub(crate) const KEY_DOWNLOADED_AT: &str = "archive_downloaded_at";
pub(crate) const KEY_EXTRACTED_AT: &str = "archive_extracted_at";
pub(crate) const KEY_ARCHIVE_FILE: &str = "archive_file";
pub(crate) const KEY_EXTRACTED_DIR: &str = "archive_extracted_dir";
pub(crate) const KEY_ETAG: &str = "archive_etag";
pub(crate) const KEY_LAST_MODIFIED: &str = "archive_last_modified";

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

    pub(crate) fn with_fetcher(
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
    pub(crate) fn attached_state(&self) -> Option<Arc<dyn PersistentStateAccess + Send + Sync>> {
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
    pub(crate) fn emit(&self, severity: ProviderMessageSeverity, message: impl AsRef<str>) {
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
        let (mut stations, channels) = parse_site_index(&site_json)?;
        for station in &mut stations {
            station_metadata::overlay(station);
        }

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
    pub(crate) fn windowed_series(
        &self,
        channel_external_id: &str,
        csvs: &[PathBuf],
        window_start: DateTime<Utc>,
        window_end: DateTime<Utc>,
    ) -> Result<
        (
            Vec<crate::core::domain::data_source::provider_port::MeasurementRecord>,
            bool,
        ),
        ProviderError,
    > {
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

    fn get_all_counting_stations(
        &self,
    ) -> Result<
        Vec<crate::core::domain::data_source::provider_port::CountingStationRecord>,
        ProviderError,
    > {
        Ok(self.ensure_archive()?.stations.clone())
    }

    fn get_all_channels(
        &self,
    ) -> Result<Vec<crate::core::domain::data_source::provider_port::ChannelRecord>, ProviderError>
    {
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

        // Advance past gaps only when data exists beyond the window. When the
        // window holds no rows and no later data exists, do NOT advance: the
        // window end is `from + timeframe`, so reporting it as the cursor would
        // jump the persisted `imported_until` watermark into the future and
        // silently skip data that arrives later.
        let last_measurement_datetime = if records.is_empty() {
            if data_beyond { Some(window_end) } else { None }
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
