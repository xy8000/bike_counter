//! Tests for the Leipzig WFS adapter: config parsing, UTM->WGS84 conversion, the
//! stations/hourly/daily parsers, the mixed-resolution join, the `DataProvider`
//! serving (incl. WFS paging and timestamp-boundary safety), cache, health and
//! provider messages.

use std::collections::{HashMap, HashSet};
use std::sync::{Arc, Mutex};

use chrono::{DateTime, NaiveDate, Utc};

use super::fetcher::ResourceFetcher;
use super::parsing::{
    DAILY_RESOLUTION_SECONDS, HOURLY_RESOLUTION_SECONDS, HourlyRow, berlin_day_bounds, build_index,
    paged_url, parse_daily_page, parse_host_and_port, parse_hourly_page, parse_stations_geojson,
    utm_zone33n_to_wgs84,
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

// Short synthetic URLs (the adapter treats them opaquely; only the paged URL
// suffix matters).
const STATIONS_URL: &str = "https://wfs.test/stations";
const HOURLY_URL: &str = "https://wfs.test/hourly?service=WFS";
const DAILY_URL: &str = "https://wfs.test/daily?service=WFS";

const MANETSTRA: &str = "de.sn.stlp.statisch.rad.100040870";
const SEMMELWEIS: &str = "de.sn.stlp.statisch.rad.100049149";

fn data_source(vars: HashMap<String, String>) -> DataSourceConfiguration {
    let provider =
        DataProviderConfiguration::new(LeipzigWfsAdapter::provider_type().to_string(), vars)
            .unwrap();
    DataSourceConfiguration::new("Leipzig".to_string(), provider).unwrap()
}

fn url_vars() -> HashMap<String, String> {
    HashMap::from([
        ("stations_url".to_string(), STATIONS_URL.to_string()),
        ("hourly_url".to_string(), HOURLY_URL.to_string()),
        ("daily_url".to_string(), DAILY_URL.to_string()),
    ])
}

fn timestamp(rfc3339: &str) -> DateTime<Utc> {
    DateTime::parse_from_rfc3339(rfc3339)
        .unwrap()
        .with_timezone(&Utc)
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

/// Fake fetcher serving bodies by exact URL; optionally failing specific URLs.
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
) -> LeipzigWfsAdapter {
    let config = data_source(vars);
    LeipzigWfsAdapter::with_fetcher(&config, fetcher).unwrap()
}

// -- fixtures ----------------------------------------------------------------

fn fixture_stations_json() -> &'static str {
    r#"{
        "type": "FeatureCollection",
        "features": [
            {"type":"Feature","geometry":{"type":"Point","coordinates":[316626.3846,5690467.4909]},"properties":{"stationid":"de.sn.stlp.statisch.rad.100040870","stationname":"Manetstraße"}},
            {"type":"Feature","geometry":{"type":"Point","coordinates":[317893.0312,5688748.02]},"properties":{"stationid":"de.sn.stlp.statisch.rad.100049149","stationname":"Semmelweisstraße"}},
            {"type":"Feature","geometry":{"type":"LineString","coordinates":[[1,2],[3,4]]},"properties":{"stationid":"de.sn.stlp.statisch.rad.999","stationname":"Ignored Geometry"}},
            {"type":"Feature","geometry":{"type":"Point","coordinates":[316626.3846,5690467.4909]},"properties":{"stationname":"No stationid"}}
        ]
    }"#
}

fn fixture_hourly_page(start_index: usize) -> String {
    let page = if start_index == 0 {
        r#"[
            {"type":"Feature","properties":{"stationid":"de.sn.stlp.statisch.rad.100040870","stationname":"Manetstraße","phenomenontime":"2026-08-06T00:00:00+02:00","count":17}},
            {"type":"Feature","properties":{"stationid":"de.sn.stlp.statisch.rad.100040870","stationname":"Manetstraße","phenomenontime":"2026-08-06T01:00:00+02:00","count":4}}
        ]"#
    } else {
        r#"[
            {"type":"Feature","properties":{"stationid":"de.sn.stlp.statisch.rad.100040870","stationname":"Manetstraße","phenomenontime":"2026-08-06T02:00:00+02:00","count":8}},
            {"type":"Feature","properties":{"stationid":"de.sn.stlp.statisch.rad.100049149","stationname":"Semmelweisstraße","phenomenontime":"2026-08-06T00:00:00+02:00","count":5}}
        ]"#
    };
    format!(
        r#"{{"type":"FeatureCollection","numberMatched":4,"numberReturned":2,"features":{page}}}"#
    )
}

fn fixture_daily_json() -> &'static str {
    r#"{
        "type": "FeatureCollection",
        "numberMatched": 2,
        "numberReturned": 2,
        "features": [
            {"type":"Feature","properties":{"stationid":"de.sn.stlp.statisch.rad.100040870","stationname":"Manetstraße","phenomenontime":"2026-03-23","count":4568}},
            {"type":"Feature","properties":{"stationid":"de.sn.stlp.statisch.rad.100040870","stationname":"Manetstraße","phenomenontime":"2026-08-06","count":100}}
        ]
    }"#
}

/// Bodies for a page-size-2 run: two hourly pages (4 features) + one daily page
/// (2 features).
fn fixture_bodies() -> HashMap<String, String> {
    let mut bodies = HashMap::new();
    bodies.insert(
        STATIONS_URL.to_string(),
        fixture_stations_json().to_string(),
    );
    bodies.insert(
        format!("{HOURLY_URL}&count=2&startIndex=0"),
        fixture_hourly_page(0),
    );
    bodies.insert(
        format!("{HOURLY_URL}&count=2&startIndex=2"),
        fixture_hourly_page(2),
    );
    bodies.insert(
        format!("{DAILY_URL}&count=2&startIndex=0"),
        fixture_daily_json().to_string(),
    );
    bodies
}

/// Reads the whole source through `get_measurements_source` until `more` is
/// false, grouping the measurements by channel external id.
fn read_source(
    adapter: &LeipzigWfsAdapter,
    from: Option<DateTime<Utc>>,
    batch_size: usize,
) -> HashMap<String, Vec<MeasurementRecord>> {
    let mut out: HashMap<String, Vec<MeasurementRecord>> = HashMap::new();
    let mut calls = 0;
    loop {
        calls += 1;
        assert!(calls < 100, "source read must terminate");
        let page = adapter.get_measurements_source(from, batch_size).unwrap();
        for measurement in page.measurements {
            out.entry(measurement.channel_external_id)
                .or_default()
                .push(measurement.record);
        }
        if !page.more {
            break;
        }
    }
    out
}

// -- config parsing ----------------------------------------------------------

#[test]
fn rejects_missing_required_urls() {
    for missing in ["stations_url", "hourly_url", "daily_url"] {
        let mut vars = url_vars();
        vars.remove(missing);
        let config = data_source(vars);
        assert!(
            matches!(
                LeipzigWfsAdapter::new(&config),
                Err(ConfigError::InvalidFormat(_))
            ),
            "missing var {missing} must be rejected"
        );
    }
}

#[test]
fn rejects_invalid_numeric_vars() {
    for (key, value) in [
        ("max_measurement_batch_size", "not-a-number"),
        ("cache_duration", "not-a-number"),
        ("wfs_page_size", "not-a-number"),
    ] {
        let mut vars = url_vars();
        vars.insert(key.to_string(), value.to_string());
        let config = data_source(vars);
        assert!(
            matches!(
                LeipzigWfsAdapter::new(&config),
                Err(ConfigError::InvalidFormat(_))
            ),
            "var {key} with '{value}' must be rejected"
        );
    }
}

#[test]
fn defaults_and_custom_values() {
    let adapter = adapter_with(url_vars(), Arc::new(FakeFetcher::new(HashMap::new())));
    assert_eq!(adapter.max_measurement_batch_size(), 500);
    assert_eq!(adapter.cache_duration_secs(), 300);
    assert_eq!(adapter.wfs_page_size(), 5000);

    let mut vars = url_vars();
    vars.insert("max_measurement_batch_size".to_string(), "100".to_string());
    vars.insert("cache_duration".to_string(), "60".to_string());
    vars.insert("wfs_page_size".to_string(), "3".to_string());
    let adapter = adapter_with(vars, Arc::new(FakeFetcher::new(HashMap::new())));
    assert_eq!(adapter.max_measurement_batch_size(), 100);
    assert_eq!(adapter.cache_duration_secs(), 60);
    assert_eq!(adapter.wfs_page_size(), 3);
}

// -- UTM conversion ----------------------------------------------------------

#[test]
fn utm_zone33n_to_wgs84_identity_at_equator_central_meridian() {
    // The UTM grid origin of zone 33N is exact: (500000, 0) is the equator at
    // the central meridian (15°E).
    let (lat, lon) = utm_zone33n_to_wgs84(500_000.0, 0.0).unwrap();
    assert!((lat - 0.0).abs() < 1e-9);
    assert!((lon - 15.0).abs() < 1e-9);
}

#[test]
fn utm_zone33n_to_wgs84_matches_known_leipzig_stations() {
    // Cross-checked against the official Leipzig locations (Eco-Visio catalog):
    // Manetstraße ~ (51.3356, 12.3676), Semmelweisstraße ~ (51.3207, 12.3865).
    // Independent catalog entries differ by ~30 m (~0.0003°), so a ~55 m
    // tolerance still catches a wrong zone / axis order / missing offset (each
    // would be off by degrees) while confirming sub-50 m projection accuracy.
    let (manet_lat, manet_lon) = utm_zone33n_to_wgs84(316626.3846, 5690467.4909).unwrap();
    assert!((manet_lat - 51.3357).abs() < 0.0005, "lat {manet_lat}");
    assert!((manet_lon - 12.3676).abs() < 0.0005, "lon {manet_lon}");

    let (semmel_lat, semmel_lon) = utm_zone33n_to_wgs84(317893.0312, 5688748.02).unwrap();
    assert!((semmel_lat - 51.3207).abs() < 0.0005, "lat {semmel_lat}");
    assert!((semmel_lon - 12.3865).abs() < 0.0005, "lon {semmel_lon}");
}

#[test]
fn utm_zone33n_to_wgs84_rejects_non_finite() {
    assert!(utm_zone33n_to_wgs84(f64::NAN, 0.0).is_none());
    assert!(utm_zone33n_to_wgs84(0.0, f64::INFINITY).is_none());
}

// -- station parser ----------------------------------------------------------

#[test]
fn parse_stations_geojson_extracts_and_converts_coordinates() {
    let sink = RecordingSink::default();
    let stations = parse_stations_geojson(fixture_stations_json(), Some(&sink)).unwrap();
    // The three features with a stationid are imported; "No stationid" is skipped.
    assert_eq!(stations.len(), 3);

    let manet = stations
        .iter()
        .find(|s| s.external_id == MANETSTRA)
        .unwrap();
    assert_eq!(manet.name, "Manetstraße");
    assert_eq!(manet.timezone, "Europe/Berlin");
    assert!((manet.latitude.unwrap() - 51.3357).abs() < 0.001);
    assert!((manet.longitude.unwrap() - 12.3676).abs() < 0.001);
    assert_eq!(manet.image_sha256, None);

    // Non-point geometry -> coordinates "not provided", station still imported.
    let ignored = stations
        .iter()
        .find(|s| s.external_id == "de.sn.stlp.statisch.rad.999")
        .unwrap();
    assert_eq!(ignored.name, "Ignored Geometry");
    assert_eq!(ignored.latitude, None);
    assert_eq!(ignored.longitude, None);

    // The feature without a stationid was skipped with a DEBUG event.
    assert!(
        sink.events
            .lock()
            .unwrap()
            .iter()
            .any(|(severity, _)| *severity == ProviderMessageSeverity::Debug)
    );
}

// -- hourly / daily parsers --------------------------------------------------

#[test]
fn parse_hourly_page_converts_rfc3339_and_reports_pagination() {
    let (rows, info) = parse_hourly_page(&fixture_hourly_page(0), None).unwrap();
    assert_eq!(rows.len(), 2);
    assert_eq!(rows[0].stationid, MANETSTRA);
    assert_eq!(rows[0].value, 17);
    // 2026-08-06T00:00:00+02:00 (CEST) -> 2026-08-05T22:00:00Z.
    assert_eq!(rows[0].timestamp.to_rfc3339(), "2026-08-05T22:00:00+00:00");
    assert_eq!(rows[1].value, 4);
    assert_eq!(rows[1].timestamp.to_rfc3339(), "2026-08-05T23:00:00+00:00");

    assert_eq!(info.number_matched, Some(4));
    assert_eq!(info.number_returned, 2);
    assert_eq!(info.next_start_index(0, 2), Some(2));
    assert_eq!(info.next_start_index(2, 2), None);
}

#[test]
fn parse_daily_page_and_page_info_fallback() {
    let (rows, info) = parse_daily_page(fixture_daily_json(), None).unwrap();
    assert_eq!(rows.len(), 2);
    assert_eq!(rows[0].date, NaiveDate::from_ymd_opt(2026, 3, 23).unwrap());
    assert_eq!(rows[0].value, 4568);
    assert_eq!(info.number_matched, Some(2));
    assert_eq!(info.next_start_index(0, 2), None, "single page covers all");

    // A page without `numberMatched`: continue only while a full page returns.
    let json = r#"{"type":"FeatureCollection","features":[{"type":"Feature","properties":{"stationid":"a","phenomenontime":"2026-03-23","count":1}}]}"#;
    let (_rows, info) = parse_daily_page(json, None).unwrap();
    assert_eq!(info.number_matched, None);
    assert_eq!(info.number_returned, 1);
    assert_eq!(
        info.next_start_index(0, 2),
        None,
        "partial page -> exhausted"
    );

    let json = r#"{"type":"FeatureCollection","features":[
        {"type":"Feature","properties":{"stationid":"a","phenomenontime":"2026-03-23","count":1}},
        {"type":"Feature","properties":{"stationid":"a","phenomenontime":"2026-03-24","count":2}}]}"#;
    let (_rows, info) = parse_daily_page(json, None).unwrap();
    assert_eq!(
        info.next_start_index(0, 2),
        Some(2),
        "full page -> keep paging"
    );
}

#[test]
fn parse_pages_skip_unusable_features() {
    let sink = RecordingSink::default();
    let json = r#"{"type":"FeatureCollection","features":[
        {"type":"Feature","properties":{"stationid":"a","phenomenontime":"not-a-time","count":1}},
        {"type":"Feature","properties":{"stationid":"a","phenomenontime":"2026-08-06T00:00:00+02:00","count":"not-a-number"}},
        {"type":"Feature","properties":{"phenomenontime":"2026-08-06T00:00:00+02:00","count":1}},
        {"type":"Feature","properties":{"stationid":"a","phenomenontime":"2026-08-06T00:00:00+02:00","count":7}}
    ]}"#;
    let (rows, info) = parse_hourly_page(json, Some(&sink)).unwrap();
    assert_eq!(rows.len(), 1, "only the fully valid feature is kept");
    assert_eq!(rows[0].value, 7);
    assert_eq!(
        info.number_returned, 4,
        "page info counts features, not rows"
    );
    // Each unusable feature emitted a DEBUG event.
    assert!(sink.events.lock().unwrap().len() >= 3);
}

#[test]
fn parse_pages_reject_invalid_json() {
    assert!(matches!(
        parse_hourly_page("not json", None),
        Err(ProviderError::InvalidData(_))
    ));
    assert!(matches!(
        parse_daily_page("not json", None),
        Err(ProviderError::InvalidData(_))
    ));
}

#[test]
fn berlin_day_bounds_are_dst_aware() {
    // Winter day (CET = UTC+1).
    let (start, end) = berlin_day_bounds(NaiveDate::from_ymd_opt(2026, 3, 23).unwrap()).unwrap();
    assert_eq!(start.to_rfc3339(), "2026-03-22T23:00:00+00:00");
    assert_eq!(end.to_rfc3339(), "2026-03-23T23:00:00+00:00");

    // Summer day (CEST = UTC+2).
    let (start, end) = berlin_day_bounds(NaiveDate::from_ymd_opt(2026, 8, 6).unwrap()).unwrap();
    assert_eq!(start.to_rfc3339(), "2026-08-05T22:00:00+00:00");
    assert_eq!(end.to_rfc3339(), "2026-08-06T22:00:00+00:00");

    // Spring-forward day 2026-03-29 is only 23 h long.
    let (start, end) = berlin_day_bounds(NaiveDate::from_ymd_opt(2026, 3, 29).unwrap()).unwrap();
    assert_eq!(start.to_rfc3339(), "2026-03-28T23:00:00+00:00");
    assert_eq!(end.to_rfc3339(), "2026-03-29T22:00:00+00:00");
    assert_eq!((end - start).num_hours(), 23);

    // Fall-back day 2026-10-25 is 25 h long.
    let (start, end) = berlin_day_bounds(NaiveDate::from_ymd_opt(2026, 10, 25).unwrap()).unwrap();
    assert_eq!(start.to_rfc3339(), "2026-10-24T22:00:00+00:00");
    assert_eq!(end.to_rfc3339(), "2026-10-25T23:00:00+00:00");
    assert_eq!((end - start).num_hours(), 25);
}

#[test]
fn paged_url_uses_the_right_separator() {
    assert_eq!(
        paged_url("https://wfs.test/layer?service=WFS", 100, 0),
        "https://wfs.test/layer?service=WFS&count=100&startIndex=0"
    );
    assert_eq!(
        paged_url("https://wfs.test/layer", 100, 200),
        "https://wfs.test/layer?count=100&startIndex=200"
    );
}

// -- join --------------------------------------------------------------------

#[test]
fn build_index_merges_both_resolutions_per_channel() {
    let stations = parse_stations_geojson(fixture_stations_json(), None).unwrap();
    let t1 = timestamp("2026-08-05T22:00:00Z");
    let hourly = vec![
        HourlyRow {
            stationid: MANETSTRA.to_string(),
            timestamp: t1,
            value: 17,
        },
        HourlyRow {
            stationid: MANETSTRA.to_string(),
            timestamp: t1,
            value: 99,
        },
    ];
    let daily = vec![crate::adapter::driven::leipzig_wfs::parsing::DailyRow {
        stationid: MANETSTRA.to_string(),
        date: NaiveDate::from_ymd_opt(2026, 8, 6).unwrap(),
        value: 100,
    }];

    let index = build_index(stations, hourly, daily, None);
    assert_eq!(index.channels.len(), 3);
    let records = index.rows.get(MANETSTRA).expect("Manetstraße has data");
    assert_eq!(
        records.len(),
        2,
        "a genuine hourly duplicate is deduped, the same-instant daily row survives"
    );
    // Sort is ascending; both rows share 2026-08-05T22:00Z but differ in resolution.
    assert_eq!(records[0].resolution_seconds, HOURLY_RESOLUTION_SECONDS);
    assert_eq!(
        records[0].value, 99,
        "duplicate hourly keeps the last value"
    );
    assert_eq!(records[1].resolution_seconds, DAILY_RESOLUTION_SECONDS);
    assert_eq!(records[1].value, 100);
    assert_eq!(
        records[1].interval_end.map(|t| t.to_rfc3339()),
        Some("2026-08-06T22:00:00+00:00".to_string())
    );
}

#[test]
fn build_index_warns_for_unmatched_station_measurements() {
    let stations = parse_stations_geojson(fixture_stations_json(), None).unwrap();
    let sink = RecordingSink::default();
    let hourly = vec![HourlyRow {
        stationid: "de.sn.stlp.statisch.rad.UNKNOWN".to_string(),
        timestamp: timestamp("2026-08-05T22:00:00Z"),
        value: 1,
    }];
    let index = build_index(stations, hourly, Vec::new(), Some(&sink));
    assert!(
        index.rows.is_empty(),
        "unmatched rows produce no measurements"
    );
    assert!(
        sink.events
            .lock()
            .unwrap()
            .iter()
            .any(|(severity, _)| *severity == ProviderMessageSeverity::Warning)
    );
}

// -- adapter serving ---------------------------------------------------------

#[test]
fn serves_stations_channels_and_measurements_with_paging() {
    let fetcher = Arc::new(FakeFetcher::new(fixture_bodies()));
    let mut vars = url_vars();
    vars.insert("wfs_page_size".to_string(), "2".to_string());
    let adapter = adapter_with(vars, fetcher);

    let stations = adapter.get_all_counting_stations().unwrap();
    assert_eq!(stations.len(), 3);

    let channels = adapter.get_all_channels().unwrap();
    assert_eq!(channels.len(), 3);
    assert!(channels.iter().any(|c| c.external_id == MANETSTRA));

    // Manetstraße: 1 winter daily row + 3 hourly rows + 1 same-instant summer
    // daily row = 5 rows ascending.
    let all = read_source(&adapter, None, 1);
    let rows = all.get(MANETSTRA).expect("Manetstraße has data");
    let summary: Vec<(String, i64, i64)> = rows
        .iter()
        .map(|r| (r.timestamp.to_rfc3339(), r.value, r.resolution_seconds))
        .collect();
    assert_eq!(
        summary,
        vec![
            (
                "2026-03-22T23:00:00+00:00".to_string(),
                4568,
                DAILY_RESOLUTION_SECONDS
            ),
            (
                "2026-08-05T22:00:00+00:00".to_string(),
                17,
                HOURLY_RESOLUTION_SECONDS
            ),
            (
                "2026-08-05T22:00:00+00:00".to_string(),
                100,
                DAILY_RESOLUTION_SECONDS
            ),
            (
                "2026-08-05T23:00:00+00:00".to_string(),
                4,
                HOURLY_RESOLUTION_SECONDS
            ),
            (
                "2026-08-06T00:00:00+00:00".to_string(),
                8,
                HOURLY_RESOLUTION_SECONDS
            ),
        ],
        "all five rows served despite batch size 1 (shared timestamp not split)"
    );

    // Semmelweisstraße: one hourly row.
    let rows = all.get(SEMMELWEIS).expect("Semmelweisstraße has data");
    assert_eq!(rows.len(), 1);
    assert_eq!(rows[0].value, 5);
}

#[test]
fn get_measurements_source_from_is_exclusive() {
    let fetcher = Arc::new(FakeFetcher::new(fixture_bodies()));
    let mut vars = url_vars();
    vars.insert("wfs_page_size".to_string(), "2".to_string());
    let adapter = adapter_with(vars, fetcher);

    // `from` is exclusive: skip the first daily row.
    let from = timestamp("2026-03-22T23:00:00Z");
    let all = read_source(&adapter, Some(from), 500);
    let rows = all.get(MANETSTRA).expect("Manetstraße has data");
    let values: Vec<i64> = rows.iter().map(|r| r.value).collect();
    assert_eq!(values, vec![17, 100, 4, 8]);
}

#[test]
fn empty_window_does_not_advance_the_cursor() {
    let fetcher = Arc::new(FakeFetcher::new(fixture_bodies()));
    let mut vars = url_vars();
    vars.insert("wfs_page_size".to_string(), "2".to_string());
    let adapter = adapter_with(vars, fetcher);

    let far_future = timestamp("2030-01-01T00:00:00Z");
    let mut saw_rows = 0;
    let mut next_from = None;
    let mut calls = 0;
    loop {
        calls += 1;
        assert!(calls < 100, "source read must terminate");
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
fn reuses_a_fresh_cache_and_refetches_when_stale() {
    let bodies = fixture_bodies();
    let fetcher = Arc::new(FakeFetcher::new(bodies.clone()));
    let mut vars = url_vars();
    vars.insert("wfs_page_size".to_string(), "2".to_string());
    let adapter = adapter_with(vars, fetcher.clone());

    adapter.get_all_counting_stations().unwrap();
    adapter.get_all_counting_stations().unwrap();
    adapter.get_all_channels().unwrap();
    // stations + 2 hourly pages + 1 daily page, fetched once.
    assert_eq!(fetcher.calls(), 4);

    // Stale (cache_duration 0) -> every access refreshes everything.
    let fetcher = Arc::new(FakeFetcher::new(bodies));
    let mut vars = url_vars();
    vars.insert("wfs_page_size".to_string(), "2".to_string());
    vars.insert("cache_duration".to_string(), "0".to_string());
    let adapter = adapter_with(vars, fetcher.clone());
    adapter.get_all_counting_stations().unwrap();
    adapter.get_all_counting_stations().unwrap();
    assert_eq!(fetcher.calls(), 8, "2 refreshes x 4 fetches");
}

#[test]
fn reports_down_for_unreachable_host() {
    let mut vars = url_vars();
    vars.insert(
        "stations_url".to_string(),
        "http://127.0.0.1:1/stations".to_string(),
    );
    let adapter = adapter_with(vars, Arc::new(FakeFetcher::new(HashMap::new())));
    assert!(matches!(adapter.check_health(), HealthStatus::Down(_)));
}

#[test]
fn attach_provider_messages_and_emit_lifecycle() {
    let fetcher = Arc::new(FakeFetcher::new(fixture_bodies()));
    let mut vars = url_vars();
    vars.insert("wfs_page_size".to_string(), "2".to_string());
    let adapter = adapter_with(vars, fetcher);
    let sink = Arc::new(RecordingSink::default());
    adapter.attach_provider_messages(sink.clone());
    adapter.emit(ProviderMessageSeverity::Info, "leipzig data refreshed");

    adapter.get_all_counting_stations().unwrap();
    let events = sink.events.lock().unwrap();
    assert!(
        events
            .iter()
            .any(|(_, message)| message.starts_with("leipzig data refreshed"))
    );
}

#[test]
fn unreachable_stations_fail_the_refresh() {
    let fetcher = FakeFetcher::new(fixture_bodies());
    fetcher
        .failing
        .lock()
        .unwrap()
        .insert(STATIONS_URL.to_string());
    let adapter = adapter_with(url_vars(), Arc::new(fetcher));
    assert!(matches!(
        adapter.get_all_counting_stations(),
        Err(ProviderError::Unreachable(_))
    ));
}

#[test]
fn parse_host_and_port_handles_the_wfs_urls() {
    assert_eq!(
        parse_host_and_port("https://geodienste.leipzig.de/l3/OpenData/wfs?service=WFS"),
        Some(("geodienste.leipzig.de".to_string(), 443))
    );
    assert_eq!(
        parse_host_and_port("http://example.com:8080/wfs"),
        Some(("example.com".to_string(), 8080))
    );
}
