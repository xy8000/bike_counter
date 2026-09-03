//! Tests for the Münster GitHub adapter: config parsing, parsers, the archive
//! cache tiers, measurement paging, and provider-message emission.

use std::collections::HashMap;
use std::fs;
use std::io::Write;
use std::sync::{Arc, Mutex};

use super::adapter::{KEY_ARCHIVE_FILE, KEY_DOWNLOADED_AT, KEY_EXTRACTED_AT, KEY_EXTRACTED_DIR};
use super::archive::{ARCHIVE_ROOT, SITE_INDEX_FILE};
use super::fetcher::{ArchiveFetcher, UpstreamHeaders};
use super::parsing::{
    berlin_to_utc, csv_month_range, parse_host_and_port, parse_measurement_csv, parse_site_index,
};
use super::*;
use crate::core::domain::configuration::configuration::value_objects::{
    DataProviderConfiguration, DataSourceConfiguration,
};
use crate::core::domain::configuration::error::ConfigError;
use crate::core::domain::data_source::provider_message::ProviderMessageSeverity;
use crate::core::domain::data_source::provider_port::{
    DataProvider, MeasurementRecord, PersistentStateAccess, ProviderError, ProviderMessageSink,
};
use crate::core::domain::health::HealthStatus;
use uuid::Uuid;

fn data_source(vars: HashMap<String, String>) -> DataSourceConfiguration {
    let provider =
        DataProviderConfiguration::new(MuensterGithubAdapter::provider_type().to_string(), vars)
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
fn parse_site_index_deduplicates_channel_names_within_a_station() {
    let json = r#"[
        {
            "name": "Bohlweg",
            "directory": "300037926",
            "start": 2023,
            "channels": [
                [300037926, "Bohlweg"],
                [353413831, "Bohlweg Fahrräder Stadteinwärts"],
                [353484923, "Bohlweg Fahrräder Stadteinwärts"],
                [353484927, "Bohlweg Fahrräder Stadteinwärts"]
            ]
        }
    ]"#;

    let (stations, channels) = parse_site_index(json).unwrap();

    assert_eq!(stations.len(), 1);
    assert_eq!(stations[0].name, "Bohlweg");
    assert_eq!(channels.len(), 3, "the aggregate entry must be skipped");

    let names: Vec<&str> = channels
        .iter()
        .map(|channel| channel.name.as_str())
        .collect();
    assert_eq!(
        names,
        vec![
            "Bohlweg Fahrräder Stadteinwärts",
            "Bohlweg Fahrräder Stadteinwärts (353484923)",
            "Bohlweg Fahrräder Stadteinwärts (353484927)"
        ]
    );
}

#[test]
fn parse_site_index_keeps_identical_channel_names_in_different_stations() {
    // Channel-name uniqueness is scoped per counting station: the same name in
    // two different stations must NOT be renamed.
    let json = r#"[
        {
            "name": "Bohlweg",
            "directory": "300037926",
            "start": 2023,
            "channels": [
                [300037926, "Bohlweg"],
                [353413831, "Bohlweg Fahrräder Stadteinwärts"]
            ]
        },
        {
            "name": "Gasselstiege",
            "directory": "300037931",
            "start": 2023,
            "channels": [
                [300037931, "Gasselstiege"],
                [353413846, "Gasselstiege Fahrräder Stadteinwärts"]
            ]
        }
    ]"#;

    let (_, channels) = parse_site_index(json).unwrap();

    let names: Vec<&str> = channels
        .iter()
        .map(|channel| channel.name.as_str())
        .collect();
    assert_eq!(
        names,
        vec![
            "Bohlweg Fahrräder Stadteinwärts",
            "Gasselstiege Fahrräder Stadteinwärts"
        ]
    );
}

#[test]
fn parse_site_index_deduplicates_station_names_across_the_archive() {
    let json = r#"[
        {
            "name": "Promenade",
            "directory": "100031297",
            "start": 2023,
            "channels": [[100031297, "Promenade"]]
        },
        {
            "name": "Promenade",
            "directory": "300037405",
            "start": 2023,
            "channels": [[300037405, "Promenade"]]
        }
    ]"#;

    let (stations, _) = parse_site_index(json).unwrap();

    assert_eq!(stations.len(), 2);
    assert_eq!(stations[0].name, "Promenade");
    assert_eq!(stations[1].name, "Promenade (300037405)");
}

#[test]
fn berlin_to_utc_handles_winter_time() {
    let naive =
        chrono::NaiveDateTime::parse_from_str("2024-01-15 12:00", "%Y-%m-%d %H:%M").unwrap();
    assert_eq!(
        berlin_to_utc(naive).unwrap().to_rfc3339(),
        "2024-01-15T11:00:00+00:00"
    );
}

#[test]
fn berlin_to_utc_handles_summer_time() {
    let naive =
        chrono::NaiveDateTime::parse_from_str("2024-07-15 12:00", "%Y-%m-%d %H:%M").unwrap();
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
        csv_month_range(std::path::Path::new("2023-01.csv")),
        Some((
            chrono::NaiveDate::from_ymd_opt(2023, 1, 1).unwrap(),
            chrono::NaiveDate::from_ymd_opt(2023, 2, 1).unwrap()
        ))
    );
    assert_eq!(
        csv_month_range(std::path::Path::new("2023-12.csv")),
        Some((
            chrono::NaiveDate::from_ymd_opt(2023, 12, 1).unwrap(),
            chrono::NaiveDate::from_ymd_opt(2024, 1, 1).unwrap()
        ))
    );
    assert_eq!(csv_month_range(std::path::Path::new("readme.md")), None);
    assert_eq!(csv_month_range(std::path::Path::new("2023-13.csv")), None);
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

    fn get(&self, _url: &str, target: &std::path::Path) -> Result<UpstreamHeaders, String> {
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
fn write_extracted_fixture(dir: &std::path::Path) {
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

/// Reads the whole source through [`DataProvider::get_measurements_source`],
/// collecting the measurements served for one channel (external id).
fn read_channel(
    adapter: &MuensterGithubAdapter,
    from: Option<chrono::DateTime<chrono::Utc>>,
    batch_size: usize,
    channel_external_id: &str,
) -> Vec<MeasurementRecord> {
    let mut out = Vec::new();
    let mut calls = 0;
    loop {
        calls += 1;
        assert!(calls < 50, "source read must terminate");
        let page = adapter.get_measurements_source(from, batch_size).unwrap();
        out.extend(
            page.measurements
                .into_iter()
                .filter(|m| m.channel_external_id == channel_external_id)
                .map(|m| m.record),
        );
        if !page.more {
            break;
        }
    }
    out
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
        .store(KEY_EXTRACTED_AT, &chrono::Utc::now().to_rfc3339())
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

    // Read the whole source and collect the second channel's measurements,
    // paged in twos: values are ascending 1, 4, 8.
    let rows = read_channel(&adapter, None, 2, "102031297");
    let values: Vec<i64> = rows.iter().map(|row| row.value).collect();
    assert_eq!(values, vec![1, 4, 8]);
    assert_eq!(rows[0].timestamp.to_rfc3339(), "2022-12-31T23:00:00+00:00");
    assert_eq!(rows[1].timestamp.to_rfc3339(), "2022-12-31T23:15:00+00:00");

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
        .store(KEY_EXTRACTED_AT, &chrono::Utc::now().to_rfc3339())
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

    // Read the whole source: with a 48h window the scanner skips the ~3-day
    // gap and still reaches the last day.
    let rows = read_channel(&adapter, None, 500, "102031297");
    let values: Vec<i64> = rows.iter().map(|row| row.value).collect();
    assert_eq!(values, vec![1, 2, 9], "gap-skip must reach the last day");

    fs::remove_dir_all(&fixture).unwrap();
}

#[test]
fn get_measurements_does_not_advance_past_the_last_data() {
    // Regression: when the incremental watermark already sits on the last real
    // sample and no new data (and no later file) exists, the cursor must NOT
    // jump to `from + timeframe`. Previously the empty window reported the
    // window end, so `imported_until` advanced into the future and silently
    // skipped data that arrives later.
    let fixture = std::env::temp_dir().join(format!("fixture-{}", Uuid::new_v4()));
    let root = fixture.join(ARCHIVE_ROOT);
    fs::create_dir_all(root.join("100031297")).unwrap();
    fs::write(root.join(SITE_INDEX_FILE), fixture_site_json()).unwrap();
    fs::write(
        root.join("100031297/2023-01.csv"),
        concat!(
            "Datetime,100031297 (Promenade),101031297 (Radfahrer),102031297 (Radfahrer),100031297-status,101031297-status,102031297-status\n",
            "2023-01-01 00:00,3,5,1,0,0,0\n",
            "2023-01-05 00:00,3,5,9,0,0,0\n",
        ),
    )
    .unwrap();

    let state = Arc::new(InMemoryAccess::default());
    state
        .store(KEY_EXTRACTED_DIR, &fixture.to_string_lossy())
        .unwrap();
    state
        .store(KEY_EXTRACTED_AT, &chrono::Utc::now().to_rfc3339())
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

    // The watermark sits on the last real sample (2023-01-05 00:00 Berlin =
    // 2023-01-04 23:00 UTC). Nothing follows it, so no cursor advance.
    let last_sample = chrono::DateTime::parse_from_rfc3339("2023-01-04T23:00:00Z")
        .unwrap()
        .with_timezone(&chrono::Utc);
    let mut saw_rows = 0;
    let mut next_from = None;
    let mut calls = 0;
    loop {
        calls += 1;
        assert!(calls < 20, "source read must terminate");
        let page = adapter
            .get_measurements_source(Some(last_sample), 500)
            .unwrap();
        saw_rows += page.measurements.len();
        if page.next_from.is_some() {
            next_from = page.next_from;
        }
        if !page.more {
            break;
        }
    }
    assert_eq!(saw_rows, 0);
    assert_eq!(next_from, None, "an empty read must not fabricate a cursor");

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

    let window_start = chrono::DateTime::parse_from_rfc3339("2023-01-14T00:00:00Z")
        .unwrap()
        .with_timezone(&chrono::Utc);
    let window_end = chrono::DateTime::parse_from_rfc3339("2023-01-16T00:00:00Z")
        .unwrap()
        .with_timezone(&chrono::Utc);
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
        .store(KEY_DOWNLOADED_AT, &chrono::Utc::now().to_rfc3339())
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
    let stale = chrono::Utc::now() - chrono::Duration::hours(2);
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
fn missing_channel_column_emits_debug_and_returns_empty_batch() {
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
    assert_eq!(events[0].0, ProviderMessageSeverity::Debug);
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
    let stale = chrono::Utc::now() - chrono::Duration::hours(2);
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
            *severity == ProviderMessageSeverity::Info && message.starts_with("archive downloaded:")
        }),
        "expected an INFO 'archive downloaded' one-liner, got: {:?}",
        *events
    );
    assert!(
        events.iter().any(|(severity, message)| {
            *severity == ProviderMessageSeverity::Info && message.starts_with("archive extracted:")
        }),
        "expected an INFO 'archive extracted' one-liner, got: {:?}",
        *events
    );
}
