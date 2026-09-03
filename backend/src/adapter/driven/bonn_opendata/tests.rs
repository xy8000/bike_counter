//! Tests for the Bonn Open Data adapter: config parsing, the GeoJSON / Vortag /
//! wide-yearly parsers, the join (aggregate exclusion, name aliasing, Vortag
//! precedence), the `DataProvider` serving, the cache and provider messages.

use std::collections::{HashMap, HashSet};
use std::sync::{Arc, Mutex};

use super::fetcher::ResourceFetcher;
use super::parsing::{
    CsvRow, WideRow, berlin_to_utc, build_index, drop_aggregate_stations, is_aggregate,
    normalize_column_name, parse_german_datetime, parse_host_and_port, parse_measurements_csv,
    parse_stations_geojson, parse_yearly_hourly_csv,
};
use super::*;
use crate::core::domain::configuration::configuration::value_objects::{
    DataProviderConfiguration, DataSourceConfiguration,
};
use crate::core::domain::configuration::error::ConfigError;
use crate::core::domain::data_source::provider_message::ProviderMessageSeverity;
use crate::core::domain::data_source::provider_port::{
    DataProvider, MeasurementRecord, ProviderError, ProviderMessageSink,
};
use crate::core::domain::health::HealthStatus;

const STATIONS_URL: &str = "https://stadtplan.bonn.de/geojson?Thema=22640";
const VORTAG_URL: &str = "https://stadtplan.bonn.de/csv?OD=4285";
const HIST_2024_URL: &str = "https://opendata.bonn.de/files/2024.csv";
const HIST_2025_URL: &str = "https://opendata.bonn.de/files/2025.csv";

fn data_source(vars: HashMap<String, String>) -> DataSourceConfiguration {
    let provider =
        DataProviderConfiguration::new(BonnOpendataAdapter::provider_type().to_string(), vars)
            .unwrap();
    DataSourceConfiguration::new("Bonn".to_string(), provider).unwrap()
}

fn vars_with_urls() -> HashMap<String, String> {
    let mut vars = HashMap::new();
    vars.insert("stations_url".to_string(), STATIONS_URL.to_string());
    vars.insert("measurements_url".to_string(), VORTAG_URL.to_string());
    vars
}

/// Vars with the 2024 historical file enabled.
fn vars_all() -> HashMap<String, String> {
    let mut vars = vars_with_urls();
    vars.insert("historical_urls".to_string(), HIST_2024_URL.to_string());
    vars
}

/// Fixture station GeoJSON: point stations 1/2/12/16 (16 is an aggregate),
/// a non-point station 99 and a feature without `station_nr`.
fn fixture_stations_json() -> &'static str {
    r#"{
        "type": "FeatureCollection",
        "features": [
            {"type":"Feature","geometry":{"type":"Point","coordinates":[7.1152570731,50.7390301848]},"properties":{"station_nr":1,"lage":"BN - Kennedybrücke (Nordseite)"}},
            {"type":"Feature","geometry":{"type":"Point","coordinates":[7.1059408371,50.7374853528]},"properties":{"station_nr":2,"lage":"BN - Kennedybrücke (Südseite)"}},
            {"type":"Feature","geometry":{"type":"Point","coordinates":[7.1134228228,50.7193231194]},"properties":{"station_nr":"12","lage":"BN - Straßburger Weg"}},
            {"type":"Feature","geometry":{"type":"Point","coordinates":[7.1103606063,50.7382990326]},"properties":{"station_nr":16,"lage":"BN - Kennedybrücke (errechnete Gesamtzahl)"}},
            {"type":"Feature","geometry":{"type":"LineString","coordinates":[[1,2],[3,4]]},"properties":{"station_nr":99,"lage":"BN - Ignored Geometry"}},
            {"type":"Feature","geometry":{"type":"Point","coordinates":[7.0,50.7]},"properties":{"lage":"BN - No Number"}}
        ]
    }"#
}

/// Fixture Vortag CSV: rows for station 1 and 2, an unmatched `lage`, and a row
/// with an unparsable `wann`.
fn fixture_vortag_csv() -> &'static str {
    concat!(
        "station_id;wann;wann_datum;anzahl_raeder;uhrzeit;lage\n",
        "100019809;2026-08-23T22:00:00;24.08.2026;0;00:00 Uhr;BN - Kennedybrücke (Nordseite)\n",
        "100019809;2026-08-23T23:00:00;24.08.2026;5;01:00 Uhr;BN - Kennedybrücke (Nordseite)\n",
        "100019810;2026-08-24T00:00:00;24.08.2026;7;02:00 Uhr;BN - Kennedybrücke (Südseite)\n",
        "100099999;2026-08-24T01:00:00;24.08.2026;9;03:00 Uhr;BN - Unmatched Station\n",
        "100019809;not-a-timestamp;24.08.2026;3;04:00 Uhr;BN - Kennedybrücke (Nordseite)\n",
    )
}

/// Fixture 2024 wide CSV: German month-name timestamps, an aliased column, an
/// aggregate column (`Summe`) and an unknown indexed column.
fn fixture_yearly_2024() -> &'static str {
    concat!(
        "Zeitraum;1. Januar 2024 -> 31. Dezember 2024;\n",
        "\n",
        "Time;5.01 BN - Kennedybrücke (Nordseite);5.02 BN - Kennedybrücke (Südseite) Barometer;5.12 BN - Straßburger Weg;Summe;5.99 BN - Unknown Station\n",
        "1. Jan. 2024 00:00;3;;1;8;9\n",
        "1. Jan. 2024 01:00;4;2;;12;\n",
        "1. Jul. 2024 00:00;10;11;12;13;14\n",
    )
}

/// Fixture 2025 wide CSV: numeric timestamps and bridge-total aggregate columns.
fn fixture_yearly_2025() -> &'static str {
    concat!(
        "Zeitraum;1. Januar 2025 -> 31. Dezember 2025;;;;;;;;;\n",
        "\n",
        "Time;5.01 BN - Kennedybrücke (Nordseite);5.03 BN - Nordbrücke (Südseite);Kennedybrücke;Nordbrücke;Südbrücke\n",
        "01.01.2025 00:00;20;30;40;50;60\n",
    )
}

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

/// Fake fetcher serving bodies by URL; optionally failing specific URLs.
struct FakeFetcher {
    bodies: HashMap<String, String>,
    failing: Mutex<HashSet<String>>,
    calls: Mutex<usize>,
}

impl FakeFetcher {
    fn new(bodies: HashMap<String, String>) -> Self {
        Self {
            bodies,
            failing: Mutex::new(HashSet::new()),
            calls: Mutex::new(0),
        }
    }

    fn calls(&self) -> usize {
        *self.calls.lock().unwrap()
    }
}

impl ResourceFetcher for FakeFetcher {
    fn fetch(&self, url: &str) -> Result<String, String> {
        *self.calls.lock().unwrap() += 1;
        if self.failing.lock().unwrap().contains(url) {
            return Err(format!("fetch failed for {url}"));
        }
        self.bodies
            .get(url)
            .cloned()
            .ok_or_else(|| format!("no fixture for {url}"))
    }
}

fn adapter_with(
    vars: HashMap<String, String>,
    fetcher: Arc<dyn ResourceFetcher>,
) -> BonnOpendataAdapter {
    let config = data_source(vars);
    BonnOpendataAdapter::with_fetcher(&config, fetcher).unwrap()
}

/// Reads the whole source through [`DataProvider::get_measurements_source`],
/// collecting the measurements served for one channel (external id).
fn read_channel(
    adapter: &BonnOpendataAdapter,
    from: Option<chrono::DateTime<chrono::Utc>>,
    batch_size: usize,
    channel_external_id: &str,
) -> Vec<MeasurementRecord> {
    let mut out = Vec::new();
    let mut calls = 0;
    loop {
        calls += 1;
        assert!(calls < 20, "source read must terminate");
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

fn timestamp(rfc3339: &str) -> chrono::DateTime<chrono::Utc> {
    chrono::DateTime::parse_from_rfc3339(rfc3339)
        .unwrap()
        .with_timezone(&chrono::Utc)
}

// -- config parsing ----------------------------------------------------------

#[test]
fn rejects_missing_stations_url() {
    let mut vars = HashMap::new();
    vars.insert("measurements_url".to_string(), VORTAG_URL.to_string());
    let config = data_source(vars);
    assert!(matches!(
        BonnOpendataAdapter::new(&config),
        Err(ConfigError::InvalidFormat(_))
    ));
}

#[test]
fn rejects_missing_measurements_url() {
    let mut vars = HashMap::new();
    vars.insert("stations_url".to_string(), STATIONS_URL.to_string());
    let config = data_source(vars);
    assert!(matches!(
        BonnOpendataAdapter::new(&config),
        Err(ConfigError::InvalidFormat(_))
    ));
}

#[test]
fn rejects_invalid_batch_size_and_cache_duration() {
    for (key, value) in [
        ("max_measurement_batch_size", "not-a-number"),
        ("cache_duration", "not-a-number"),
    ] {
        let mut vars = vars_with_urls();
        vars.insert(key.to_string(), value.to_string());
        let config = data_source(vars);
        assert!(
            matches!(
                BonnOpendataAdapter::new(&config),
                Err(ConfigError::InvalidFormat(_))
            ),
            "var {key} with '{value}' must be rejected"
        );
    }
}

#[test]
fn defaults_batch_size_and_cache_duration() {
    let adapter = adapter_with(vars_with_urls(), Arc::new(FakeFetcher::new(HashMap::new())));
    assert_eq!(adapter.max_measurement_batch_size(), 500);
    assert_eq!(adapter.cache_duration_secs(), 300);
}

#[test]
fn reads_batch_size_cache_duration_and_historical_urls() {
    let mut vars = vars_with_urls();
    vars.insert("max_measurement_batch_size".to_string(), "100".to_string());
    vars.insert("cache_duration".to_string(), "60".to_string());
    vars.insert(
        "historical_urls".to_string(),
        format!("{HIST_2024_URL} {HIST_2025_URL}"),
    );
    let adapter = adapter_with(vars, Arc::new(FakeFetcher::new(HashMap::new())));
    assert_eq!(adapter.max_measurement_batch_size(), 100);
    assert_eq!(adapter.cache_duration_secs(), 60);
    assert_eq!(adapter.historical_urls().len(), 2);
}

// -- parsers -----------------------------------------------------------------

#[test]
fn parse_stations_geojson_extracts_stations() {
    let stations = parse_stations_geojson(fixture_stations_json(), None).unwrap();
    assert_eq!(
        stations.len(),
        5,
        "the feature without station_nr is skipped"
    );

    let nr1 = stations.iter().find(|s| s.external_id == "1").unwrap();
    assert_eq!(nr1.name, "BN - Kennedybrücke (Nordseite)");
    assert_eq!(nr1.latitude, Some(50.7390301848));
    assert_eq!(nr1.longitude, Some(7.1152570731));
    assert_eq!(nr1.timezone, "Europe/Berlin");
    assert_eq!(nr1.image_sha256, None);

    // station_nr as a string is normalized to the same external id.
    assert!(stations.iter().any(|s| s.external_id == "12"));

    // Non-point geometry -> coordinates "not provided".
    let nr99 = stations.iter().find(|s| s.external_id == "99").unwrap();
    assert_eq!(nr99.latitude, None);
    assert_eq!(nr99.longitude, None);
}

#[test]
fn drop_aggregate_stations_removes_computed_totals() {
    let stations = parse_stations_geojson(fixture_stations_json(), None).unwrap();
    assert!(is_aggregate("BN - Kennedybrücke (errechnete Gesamtzahl)"));
    assert!(!is_aggregate("BN - Kennedybrücke (Nordseite)"));

    let kept = drop_aggregate_stations(stations);
    assert_eq!(kept.len(), 4, "the aggregate station_nr 16 is dropped");
    assert!(!kept.iter().any(|s| s.external_id == "16"));
}

#[test]
fn normalize_column_name_strips_index_and_aliases() {
    assert_eq!(
        normalize_column_name("5.01 BN - Kennedybrücke (Nordseite)"),
        "BN - Kennedybrücke (Nordseite)"
    );
    // Alias table for renamed stations.
    assert_eq!(
        normalize_column_name("5.02 BN - Kennedybrücke (Südseite) Barometer"),
        "BN - Kennedybrücke (Südseite)"
    );
    assert_eq!(
        normalize_column_name("5.10 BN - Bröhltalweg"),
        "BN - Bröltalbahnweg"
    );
    // Aggregate columns have no index prefix and are left unchanged.
    assert_eq!(normalize_column_name("Summe"), "Summe");
    assert_eq!(normalize_column_name("Kennedybrücke"), "Kennedybrücke");
}

#[test]
fn parse_german_datetime_handles_both_formats() {
    assert_eq!(
        parse_german_datetime("1. Jan. 2024 00:00").unwrap(),
        chrono::NaiveDate::from_ymd_opt(2024, 1, 1)
            .unwrap()
            .and_hms_opt(0, 0, 0)
            .unwrap()
    );
    assert_eq!(
        parse_german_datetime("31. März 2024 03:00").unwrap(),
        chrono::NaiveDate::from_ymd_opt(2024, 3, 31)
            .unwrap()
            .and_hms_opt(3, 0, 0)
            .unwrap()
    );
    assert_eq!(
        parse_german_datetime("01.01.2025 00:00").unwrap(),
        chrono::NaiveDate::from_ymd_opt(2025, 1, 1)
            .unwrap()
            .and_hms_opt(0, 0, 0)
            .unwrap()
    );
    assert!(parse_german_datetime("not a date").is_none());
}

#[test]
fn berlin_to_utc_is_dst_aware() {
    // Winter (CET = UTC+1).
    let winter = chrono::NaiveDate::from_ymd_opt(2024, 1, 15)
        .unwrap()
        .and_hms_opt(12, 0, 0)
        .unwrap();
    assert_eq!(
        berlin_to_utc(winter).unwrap().to_rfc3339(),
        "2024-01-15T11:00:00+00:00"
    );
    // Summer (CEST = UTC+2).
    let summer = chrono::NaiveDate::from_ymd_opt(2024, 7, 15)
        .unwrap()
        .and_hms_opt(12, 0, 0)
        .unwrap();
    assert_eq!(
        berlin_to_utc(summer).unwrap().to_rfc3339(),
        "2024-07-15T10:00:00+00:00"
    );
}

#[test]
fn parse_measurements_csv_parses_utc_timestamps() {
    let rows = parse_measurements_csv(fixture_vortag_csv(), None).unwrap();
    // 4 valid rows: 2 x station 100019809, 1 x 100019810, 1 x unmatched; the
    // unparsable `wann` row is skipped.
    assert_eq!(rows.len(), 4);

    let first = &rows[0];
    assert_eq!(first.station_id, "100019809");
    assert_eq!(first.value, 0);
    assert_eq!(first.timestamp.to_rfc3339(), "2026-08-23T22:00:00+00:00");
    assert_eq!(first.lage, "BN - Kennedybrücke (Nordseite)");
}

#[test]
fn parse_measurements_csv_missing_column_is_invalid_data() {
    let csv = "station_id;wann;wann_datum;uhrzeit;lage\n100019809;2026-08-23T22:00:00;24.08.2026;00:00 Uhr;BN - Nordbrücke (Südseite)\n";
    assert!(matches!(
        parse_measurements_csv(csv, None),
        Err(ProviderError::InvalidData(_))
    ));
}

#[test]
fn parse_yearly_hourly_csv_maps_columns_and_skips_aggregates() {
    let stations = parse_stations_geojson(fixture_stations_json(), None).unwrap();
    let stations = drop_aggregate_stations(stations);
    let by_name = super::parsing::station_name_map(&stations);

    let sink = RecordingSink::default();
    let rows = parse_yearly_hourly_csv(fixture_yearly_2024(), &by_name, Some(&sink)).unwrap();

    // Row 1: station 1 (3) + station 12 (1); row 2: station 1 (4) + station 2
    // (2 via alias); row 3: stations 1/2/12 (10/11/12). Aggregate + unknown
    // columns are skipped.
    let mut rows = rows;
    rows.sort_by_key(|row| (row.channel.clone(), row.timestamp));

    // Rows are sorted lexicographically by channel external id ("1" < "12" <
    // "2"), then by timestamp.
    let expected: Vec<(String, &str, i64)> = vec![
        ("1".to_string(), "2023-12-31T23:00:00+00:00", 3),
        ("1".to_string(), "2024-01-01T00:00:00+00:00", 4),
        ("1".to_string(), "2024-06-30T22:00:00+00:00", 10),
        ("12".to_string(), "2023-12-31T23:00:00+00:00", 1),
        ("12".to_string(), "2024-06-30T22:00:00+00:00", 12),
        ("2".to_string(), "2024-01-01T00:00:00+00:00", 2),
        ("2".to_string(), "2024-06-30T22:00:00+00:00", 11),
    ];
    let actual: Vec<(String, String, i64)> = rows
        .iter()
        .map(|row| (row.channel.clone(), row.timestamp.to_rfc3339(), row.value))
        .collect();
    let expected_str: Vec<(String, String, i64)> = expected
        .iter()
        .map(|(c, t, v)| (c.clone(), t.to_string(), *v))
        .collect();
    assert_eq!(actual, expected_str);

    // The unknown indexed column (5.99) is an indexed-but-unmatched column -> WARNING.
    let warnings = sink
        .events
        .lock()
        .unwrap()
        .iter()
        .filter(|(severity, _)| *severity == ProviderMessageSeverity::Warning)
        .count();
    assert_eq!(warnings, 1);
}

#[test]
fn parse_yearly_hourly_csv_handles_numeric_timestamps_and_bridge_totals() {
    let stations = parse_stations_geojson(fixture_stations_json(), None).unwrap();
    let stations = drop_aggregate_stations(stations);
    let by_name = super::parsing::station_name_map(&stations);

    let rows = parse_yearly_hourly_csv(fixture_yearly_2025(), &by_name, None).unwrap();
    // Only station 1 (20) maps; 5.03 (indexed, unknown) is skipped with a
    // warning and the bridge totals are aggregates.
    assert_eq!(rows.len(), 1);
    assert_eq!(rows[0].channel, "1");
    assert_eq!(rows[0].value, 20);
    assert_eq!(rows[0].timestamp.to_rfc3339(), "2024-12-31T23:00:00+00:00");
}

#[test]
fn parse_yearly_hourly_csv_without_time_header_is_invalid_data() {
    let csv = "Time;Summe\n1. Jan. 2024 00:00;8\n";
    // No station columns; the header still starts with Time so it parses, but
    // there is nothing to map -> empty.
    let stations = parse_stations_geojson(fixture_stations_json(), None).unwrap();
    let by_name = super::parsing::station_name_map(&stations);
    let rows = parse_yearly_hourly_csv(csv, &by_name, None).unwrap();
    assert!(rows.is_empty());

    // A file without a `Time` header row at all is a format change.
    let broken = "Zeitraum;1. Januar 2024 -> 31. Dezember 2024;\n";
    assert!(matches!(
        parse_yearly_hourly_csv(broken, &by_name, None),
        Err(ProviderError::InvalidData(_))
    ));
}

// -- join --------------------------------------------------------------------

#[test]
fn build_index_creates_one_channel_per_station_and_merges_sources() {
    let stations = parse_stations_geojson(fixture_stations_json(), None).unwrap();
    let stations = drop_aggregate_stations(stations);

    let vortag = parse_measurements_csv(fixture_vortag_csv(), None).unwrap();
    let by_name = super::parsing::station_name_map(&stations);
    let yearly = parse_yearly_hourly_csv(fixture_yearly_2024(), &by_name, None).unwrap();

    let sink = RecordingSink::default();
    let index = build_index(stations, vortag, yearly, Some(&sink));

    // 4 imported stations (1, 2, 12, 99); one channel per station keyed by
    // station_nr.
    assert_eq!(index.stations.len(), 4);
    assert_eq!(index.channels.len(), 4);
    assert!(index.channels.iter().any(|c| c.external_id == "1"));
    assert!(index.channels.iter().any(|c| c.external_id == "12"));

    // Channel 1 merges historical + vortag rows (ascending, deduped).
    let channel_1 = index.rows.get("1").expect("channel 1 has data");
    let values: Vec<i64> = channel_1.iter().map(|r| r.value).collect();
    assert_eq!(values, vec![3, 4, 10, 0, 5]);

    // The unmatched Vortag lage is skipped with a WARNING.
    let warnings = sink
        .events
        .lock()
        .unwrap()
        .iter()
        .filter(|(severity, _)| *severity == ProviderMessageSeverity::Warning)
        .count();
    assert!(warnings >= 1);
}

#[test]
fn build_index_prefers_vortag_value_on_overlapping_hours() {
    let stations = parse_stations_geojson(fixture_stations_json(), None).unwrap();
    let stations = drop_aggregate_stations(stations);
    let t = timestamp("2024-01-01T00:00:00Z");

    let vortag = vec![CsvRow {
        station_id: "100019809".to_string(),
        timestamp: t,
        value: 99,
        lage: "BN - Kennedybrücke (Nordseite)".to_string(),
    }];
    let yearly = vec![WideRow {
        channel: "1".to_string(),
        timestamp: t,
        value: 3,
    }];

    let index = build_index(stations, vortag, yearly, None);
    let channel_1 = index.rows.get("1").unwrap();
    assert_eq!(channel_1.len(), 1, "overlapping hour is deduplicated");
    assert_eq!(channel_1[0].value, 99, "the Vortag value wins");
}

#[test]
fn build_index_skips_aggregate_vortag_rows_silently() {
    // The excluded `(errechnete Gesamtzahl)` aggregate stations still appear in
    // the Vortag CSV. Their rows are expected and must be skipped without a
    // per-row WARNING (otherwise the message table floods every refresh).
    let stations = parse_stations_geojson(fixture_stations_json(), None).unwrap();
    let stations = drop_aggregate_stations(stations);
    let t = timestamp("2026-08-25T21:00:00Z");
    let vortag = vec![CsvRow {
        station_id: "100035004".to_string(),
        timestamp: t,
        value: 5,
        lage: "BN - Nordbrücke (errechnete Gesamtzahl)".to_string(),
    }];

    let sink = RecordingSink::default();
    let index = build_index(stations, vortag, Vec::new(), Some(&sink));

    assert!(
        index.rows.is_empty(),
        "no channel/data for the aggregate row"
    );
    assert!(
        sink.events.lock().unwrap().is_empty(),
        "the expected aggregate row must not emit a WARNING"
    );
}

// -- adapter serving ---------------------------------------------------------

fn fixtures() -> HashMap<String, String> {
    HashMap::from([
        (
            STATIONS_URL.to_string(),
            fixture_stations_json().to_string(),
        ),
        (VORTAG_URL.to_string(), fixture_vortag_csv().to_string()),
        (HIST_2024_URL.to_string(), fixture_yearly_2024().to_string()),
    ])
}

#[test]
fn serves_stations_channels_and_measurements() {
    let fetcher = Arc::new(FakeFetcher::new(fixtures()));
    let adapter = adapter_with(vars_all(), fetcher);

    let stations = adapter.get_all_counting_stations().unwrap();
    assert_eq!(stations.len(), 4, "aggregate station 16 is dropped");
    assert!(!stations.iter().any(|s| s.external_id == "16"));

    let channels = adapter.get_all_channels().unwrap();
    assert_eq!(channels.len(), 4);
    assert!(channels.iter().any(|c| c.external_id == "1"));

    // Channel 1: historical (3 rows) + vortag (2 rows) = 5 rows ascending.
    let rows = read_channel(&adapter, None, 500, "1");
    let values: Vec<i64> = rows.iter().map(|row| row.value).collect();
    assert_eq!(values, vec![3, 4, 10, 0, 5]);
    assert_eq!(
        rows.last().unwrap().timestamp.to_rfc3339(),
        "2026-08-23T23:00:00+00:00"
    );
}

#[test]
fn get_measurements_source_filters_from_exclusive_and_pages() {
    let fetcher = Arc::new(FakeFetcher::new(fixtures()));
    let adapter = adapter_with(vars_all(), fetcher);

    // `from` is exclusive: the 2024-06-30T22:00:00Z row (value 10) is skipped
    // and only the two later Vortag rows remain, read in pages of two.
    let from = timestamp("2024-06-30T22:00:00Z");
    let rows = read_channel(&adapter, Some(from), 2, "1");
    let values: Vec<i64> = rows.iter().map(|row| row.value).collect();
    assert_eq!(values, vec![0, 5]);
    assert_eq!(rows[0].timestamp.to_rfc3339(), "2026-08-23T22:00:00+00:00");
}

#[test]
fn get_measurements_source_empty_window_does_not_advance_the_cursor() {
    let fetcher = Arc::new(FakeFetcher::new(fixtures()));
    let adapter = adapter_with(vars_all(), fetcher);

    // A `from` beyond all data returns no rows and never fabricates a
    // watermark (otherwise `imported_until` would jump into the future).
    let far_future = timestamp("2030-01-01T00:00:00Z");
    let mut saw_rows = 0;
    let mut next_from = None;
    let mut calls = 0;
    loop {
        calls += 1;
        assert!(calls < 10, "source read must terminate");
        let batch = adapter
            .get_measurements_source(Some(far_future), 500)
            .unwrap();
        saw_rows += batch.measurements.len();
        if batch.next_from.is_some() {
            next_from = batch.next_from;
        }
        if !batch.more {
            break;
        }
    }
    assert_eq!(saw_rows, 0);
    assert_eq!(next_from, None, "an empty read must not fabricate a cursor");
}

#[test]
fn reuses_a_fresh_cache() {
    let fetcher = Arc::new(FakeFetcher::new(fixtures()));
    let adapter = adapter_with(vars_all(), fetcher.clone());

    adapter.get_all_counting_stations().unwrap();
    adapter.get_all_counting_stations().unwrap();
    adapter.get_all_channels().unwrap();
    assert_eq!(
        fetcher.calls(),
        3,
        "stations+vortag+historical fetched once"
    );
}

#[test]
fn refetches_when_the_cache_is_stale() {
    let fetcher = Arc::new(FakeFetcher::new(fixtures()));
    let mut vars = vars_all();
    vars.insert("cache_duration".to_string(), "0".to_string());
    let adapter = adapter_with(vars, fetcher.clone());

    adapter.get_all_counting_stations().unwrap();
    adapter.get_all_counting_stations().unwrap();
    // 6 calls = 2 refreshes x (stations + vortag + historical).
    assert_eq!(fetcher.calls(), 6);
}

#[test]
fn historical_fetch_failure_skips_the_year_but_keeps_importing() {
    let mut bodies = fixtures();
    bodies.remove(HIST_2024_URL);
    let fetcher = FakeFetcher::new(bodies);
    fetcher
        .failing
        .lock()
        .unwrap()
        .insert(HIST_2024_URL.to_string());
    let fetcher = Arc::new(fetcher);
    let mut vars = vars_with_urls();
    vars.insert("historical_urls".to_string(), HIST_2024_URL.to_string());
    let adapter = adapter_with(vars, fetcher);

    let sink = Arc::new(RecordingSink::default());
    adapter.attach_provider_messages(sink.clone());

    // Current stations/channels still import; the year is skipped with a WARNING.
    let channels = adapter.get_all_channels().unwrap();
    assert!(!channels.is_empty());
    let warnings = sink
        .events
        .lock()
        .unwrap()
        .iter()
        .filter(|(severity, _)| *severity == ProviderMessageSeverity::Warning)
        .count();
    assert!(warnings >= 1);
}

#[test]
fn reports_down_for_unreachable_host() {
    let mut vars = vars_with_urls();
    vars.insert(
        "measurements_url".to_string(),
        "http://127.0.0.1:1/csv?OD=4285".to_string(),
    );
    let adapter = adapter_with(vars, Arc::new(FakeFetcher::new(HashMap::new())));
    assert!(matches!(adapter.check_health(), HealthStatus::Down(_)));
}

#[test]
fn attach_provider_messages_and_emit() {
    let fetcher = Arc::new(FakeFetcher::new(fixtures()));
    let adapter = adapter_with(vars_all(), fetcher);
    let sink = Arc::new(RecordingSink::default());
    adapter.attach_provider_messages(sink.clone());
    adapter.emit(ProviderMessageSeverity::Info, "bonn data refreshed");

    // A real data access also emits the refresh INFO one-liner.
    adapter.get_all_counting_stations().unwrap();
    let events = sink.events.lock().unwrap();
    assert!(
        events
            .iter()
            .any(|(_, m)| m.starts_with("bonn data refreshed"))
    );
}

#[test]
fn parse_host_and_port_handles_bonn_urls() {
    assert_eq!(
        parse_host_and_port("https://stadtplan.bonn.de/csv?OD=4285"),
        Some(("stadtplan.bonn.de".to_string(), 443))
    );
    assert_eq!(
        parse_host_and_port("http://example.com:8080/path"),
        Some(("example.com".to_string(), 8080))
    );
}
