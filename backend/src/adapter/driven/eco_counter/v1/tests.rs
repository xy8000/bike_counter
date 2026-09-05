//! Unit tests for the V1 adapter (fixtures + fake fetcher, no network).

use std::collections::HashMap;
use std::sync::Arc;

use chrono::{DateTime, Duration, Utc};

use crate::adapter::driven::eco_counter::common::utc;
use crate::adapter::driven::eco_counter::fetcher::ResourceFetcher;
use crate::core::domain::configuration::configuration::value_objects::{
    DataProviderConfiguration, DataSourceConfiguration,
};
use crate::core::domain::data_source::provider_port::{
    ChannelRecord, DataProvider, MeasurementRecord,
};
use crate::core::domain::health::HealthStatus;

use super::adapter::EcoCounterV1Adapter;
use super::catalog::CatalogStation;

/// Reads the whole source through [`DataProvider::get_measurements_source`].
fn read_all(
    a: &EcoCounterV1Adapter,
    from: Option<DateTime<Utc>>,
) -> (Vec<MeasurementRecord>, Option<DateTime<Utc>>) {
    let mut measurements = Vec::new();
    let mut watermark = None;
    let mut calls = 0;
    loop {
        calls += 1;
        assert!(calls < 100, "source read must terminate");
        let batch = a.get_measurements_source(from, 1000).unwrap();
        for m in &batch.measurements {
            measurements.push(m.record.clone());
        }
        if let Some(next) = batch.next_from {
            watermark = Some(next);
        }
        if !batch.more {
            break;
        }
    }
    (measurements, watermark)
}

/// A fake fetcher that serves per-counter **metadata** (matched by URL substring)
/// and per-counter, per-`step` **data** JSON. The provider probes steps in order,
/// so a test models "this counter has no 15-min data" by leaving the `(id, 2)`
/// response empty (`[]`) and supplying rows under `(id, 3)`.
struct FakeFetcher {
    metadata: HashMap<String, String>,
    /// station id -> step -> response body.
    data: HashMap<i64, HashMap<i64, String>>,
}

impl FakeFetcher {
    fn new(metadata: Vec<(&str, &str)>, data: Vec<(i64, i64, &str)>) -> Self {
        let mut by_station: HashMap<i64, HashMap<i64, String>> = HashMap::new();
        for (id, step, body) in data {
            by_station
                .entry(id)
                .or_default()
                .insert(step, body.to_string());
        }
        Self {
            metadata: metadata
                .into_iter()
                .map(|(k, v)| (k.to_string(), v.to_string()))
                .collect(),
            data: by_station,
        }
    }

    fn station_id(url: &str) -> Option<i64> {
        let rest = url.split("/pbl/publicwebpage/data/").nth(1)?;
        let id = rest.split(['?', '/']).next()?;
        id.parse().ok()
    }

    fn step(url: &str) -> Option<i64> {
        let rest = url.split("step=").nth(1)?;
        let step = rest.split(['&', ' ']).next()?;
        step.parse().ok()
    }
}

impl ResourceFetcher for FakeFetcher {
    fn fetch(&self, url: &str) -> Result<String, String> {
        if url.contains("/pbl/publicwebpage/data/") {
            let id = Self::station_id(url)
                .ok_or_else(|| format!("cannot parse station id from {url}"))?;
            let step = Self::step(url).ok_or_else(|| format!("cannot parse step from {url}"))?;
            let body = self
                .data
                .get(&id)
                .and_then(|steps| steps.get(&step))
                .cloned()
                .ok_or_else(|| format!("no fake data response (station {id}, step {step})"))?;
            // A body prefixed with "ERROR:" simulates a fetch failure (e.g.
            // "http status: 400"), matching the live API's per-step rejections.
            if let Some(message) = body.strip_prefix("ERROR:") {
                return Err(message.to_string());
            }
            return Ok(body);
        }
        for (key, json) in &self.metadata {
            if url.contains(key) {
                return Ok(json.clone());
            }
        }
        Err(format!("no fake metadata response for {url}"))
    }
}

const META_STEIN: &str = r#"{"token":"81ee145d681ec7d08a28a037257117634ff718053a5e6f639948583cf3fb0f8b",
  "titre":"Stadt Stein Nürnberger Straße","idPdc":100063085,"cumulFlowId":100063085,
  "latitude":49.4163,"longitude":11.0188,"pratique":2,"domaine":7242,"date":"2020-10-01"}"#;
/// A second still-public counter (token + domaine present) used by the
/// per-channel resolution test.
const META_SECOND: &str = r#"{"token":"2222222222222222222222222222222222222222222222222222222222222222",
  "titre":"Zweiter Zähler","idPdc":100000445,"latitude":51.0,"longitude":9.0,
  "pratique":2,"domaine":1,"date":"2020-01-01"}"#;
const META_EMPTY: &str = r#"{"logos":[],"latitude":0.0,"longitude":0.0,"channels":[]}"#;

fn catalog(ids: &[i64]) -> Vec<CatalogStation> {
    ids.iter()
        .map(|id| CatalogStation {
            id: *id,
            name: None,
            latitude: None,
            longitude: None,
            timezone: None,
        })
        .collect()
}

fn config(vars: &[(&str, &str)]) -> DataSourceConfiguration {
    let vars: HashMap<String, String> = vars
        .iter()
        .map(|(k, v)| (k.to_string(), v.to_string()))
        .collect();
    let provider =
        DataProviderConfiguration::new("eco_counter_v1_http_provider".to_string(), vars).unwrap();
    DataSourceConfiguration::new("Eco-Counter".to_string(), provider).unwrap()
}

fn data_json(rows: &[(DateTime<Utc>, i64)]) -> String {
    let items: Vec<String> = rows
        .iter()
        .map(|(t, value)| {
            format!(
                "{{\"date\":\"{}\",\"comptage\":{},\"timestamp\":{}}}",
                t.format("%Y-%m-%d %H:%M:%S"),
                value,
                t.timestamp_millis()
            )
        })
        .collect();
    format!("[{}]", items.join(","))
}

/// Midnight ~9 days ago, the shared base of all fixture rows.
fn base_recent() -> DateTime<Utc> {
    let now = Utc::now();
    (now - Duration::days(9))
        .date_naive()
        .and_hms_opt(0, 0, 0)
        .map(|naive| DateTime::from_naive_utc_and_offset(naive, Utc))
        .expect("valid instant")
}

/// Three 15-minute rows (step `2`) nine days ago.
fn quarter_rows() -> Vec<(DateTime<Utc>, i64)> {
    let base = base_recent();
    vec![
        (base, 10),
        (base + Duration::minutes(15), 11),
        (base + Duration::minutes(30), 12),
    ]
}

/// Three hourly rows (step `3`) nine days ago.
fn hourly_rows() -> Vec<(DateTime<Utc>, i64)> {
    let base = base_recent();
    vec![
        (base, 10),
        (base + Duration::hours(1), 11),
        (base + Duration::hours(2), 12),
    ]
}

/// Three daily rows (step `4`) on consecutive days.
fn daily_rows() -> Vec<(DateTime<Utc>, i64)> {
    let base = base_recent();
    vec![
        (base, 10),
        (base + Duration::days(1), 11),
        (base + Duration::days(2), 12),
    ]
}

fn adapter_with(
    vars: &[(&str, &str)],
    ids: &[i64],
    meta: &[(&str, &str)],
    data: Vec<(i64, i64, &str)>,
) -> EcoCounterV1Adapter {
    EcoCounterV1Adapter::with_fetcher(
        &config(vars),
        catalog(ids),
        Arc::new(FakeFetcher::new(meta.to_vec(), data)),
    )
    .unwrap()
}

#[test]
fn provider_type_is_versioned() {
    assert_eq!(
        EcoCounterV1Adapter::provider_type(),
        "eco_counter_v1_http_provider"
    );
}

#[test]
fn discovers_station_and_channel_from_metadata() {
    let a = adapter_with(
        &[("import_days_back", "10"), ("page_days", "30")],
        &[100063085],
        &[("publicwebpage/100063085", META_STEIN)],
        vec![],
    );
    let stations = a.get_all_counting_stations().unwrap();
    assert_eq!(stations.len(), 1);
    assert_eq!(stations[0].external_id, "100063085");
    assert_eq!(stations[0].name, "Stadt Stein Nürnberger Straße");
    assert_eq!(stations[0].latitude, Some(49.4163));
    let channels: Vec<ChannelRecord> = a.get_all_channels().unwrap();
    assert_eq!(channels.len(), 1);
    assert_eq!(channels[0].external_id, "100063085");
}

#[test]
fn skips_migrated_stations_without_a_token() {
    let a = adapter_with(
        &[],
        &[100000445],
        &[("publicwebpage/100000445", META_EMPTY)],
        vec![],
    );
    assert!(a.get_all_counting_stations().is_err());
}

#[test]
fn imports_fifteen_minute_when_available() {
    let rows = quarter_rows();
    let json = data_json(&rows);
    let a = adapter_with(
        &[("import_days_back", "10"), ("page_days", "30")],
        &[100063085],
        &[("publicwebpage/100063085", META_STEIN)],
        vec![(100063085, 2, json.as_str())],
    );
    let (measurements, watermark) = read_all(&a, None);
    assert_eq!(measurements.len(), 3);
    assert_eq!(measurements[0].resolution_seconds, 900);
    assert_eq!(measurements[0].value, 10);
    assert_eq!(watermark.expect("a watermark after a full read"), rows[2].0);
}

#[test]
fn falls_back_to_hourly_when_fifteen_minute_is_empty() {
    let rows = hourly_rows();
    let json = data_json(&rows);
    let a = adapter_with(
        &[("import_days_back", "10"), ("page_days", "30")],
        &[100063085],
        &[("publicwebpage/100063085", META_STEIN)],
        vec![(100063085, 2, "[]"), (100063085, 3, json.as_str())],
    );
    let (measurements, watermark) = read_all(&a, None);
    assert_eq!(measurements.len(), 3);
    assert_eq!(measurements[0].resolution_seconds, 3600);
    assert_eq!(measurements[0].value, 10);
    assert_eq!(measurements[1].value, 11);
    assert_eq!(measurements[2].value, 12);
    assert_eq!(watermark.expect("a watermark after a full read"), rows[2].0);
}

#[test]
fn falls_back_to_daily_when_finer_resolutions_are_empty() {
    let rows = daily_rows();
    let json = data_json(&rows);
    let a = adapter_with(
        &[("import_days_back", "10"), ("page_days", "30")],
        &[100063085],
        &[("publicwebpage/100063085", META_STEIN)],
        vec![
            (100063085, 2, "[]"),
            (100063085, 3, "[]"),
            (100063085, 4, json.as_str()),
        ],
    );
    let (measurements, watermark) = read_all(&a, None);
    assert_eq!(measurements.len(), 3);
    assert_eq!(measurements[0].resolution_seconds, 86_400);
    assert_eq!(watermark.expect("a watermark after a full read"), rows[2].0);
}

#[test]
fn keeps_the_finest_step_when_no_resolution_returns_data() {
    let a = adapter_with(
        &[("import_days_back", "10"), ("page_days", "30")],
        &[100063085],
        &[("publicwebpage/100063085", META_STEIN)],
        vec![
            (100063085, 2, "[]"),
            (100063085, 3, "[]"),
            (100063085, 4, "[]"),
        ],
    );
    let (measurements, watermark) = read_all(&a, None);
    assert!(measurements.is_empty());
    assert!(watermark.is_none(), "no data -> no watermark");
}

#[test]
fn falls_back_to_hourly_when_fifteen_minute_is_rejected_with_http_400() {
    // The live API can *reject* a finer step (HTTP 4xx) instead of returning an
    // empty array; the provider must treat that as "not available" too.
    let rows = hourly_rows();
    let json = data_json(&rows);
    let a = adapter_with(
        &[("import_days_back", "10"), ("page_days", "30")],
        &[100063085],
        &[("publicwebpage/100063085", META_STEIN)],
        vec![
            (100063085, 2, "ERROR:http status: 400"),
            (100063085, 3, json.as_str()),
        ],
    );
    let (measurements, watermark) = read_all(&a, None);
    assert_eq!(measurements.len(), 3);
    assert_eq!(measurements[0].resolution_seconds, 3600);
    assert_eq!(watermark.expect("a watermark after a full read"), rows[2].0);
}

#[test]
fn skips_a_station_that_rejects_every_step_with_http_400() {
    let a = adapter_with(
        &[("import_days_back", "10"), ("page_days", "30")],
        &[100063085],
        &[("publicwebpage/100063085", META_STEIN)],
        vec![
            (100063085, 2, "ERROR:http status: 400"),
            (100063085, 3, "ERROR:http status: 400"),
            (100063085, 4, "ERROR:http status: 400"),
        ],
    );
    let (measurements, watermark) = read_all(&a, None);
    assert!(measurements.is_empty(), "unserved station imports nothing");
    assert!(
        watermark.is_none(),
        "a station served by no step must not advance the watermark"
    );
}

#[test]
fn propagates_a_transport_error_during_probing() {
    // Only HTTP 4xx means "step not available"; a transport failure must abort.
    let a = adapter_with(
        &[("import_days_back", "10"), ("page_days", "30")],
        &[100063085],
        &[("publicwebpage/100063085", META_STEIN)],
        vec![(100063085, 2, "ERROR:connection refused")],
    );
    assert!(a.get_measurements_source(None, 1000).is_err());
}

#[test]
fn prefers_the_finest_available_resolution_per_channel() {
    let quarter = data_json(&quarter_rows());
    let hourly = data_json(&hourly_rows());
    let a = adapter_with(
        &[("import_days_back", "10"), ("page_days", "30")],
        &[100063085, 100000445],
        &[
            ("publicwebpage/100063085", META_STEIN),
            ("publicwebpage/100000445", META_SECOND),
        ],
        vec![
            // Station A has 15-min data; station B only hourly.
            (100063085, 2, quarter.as_str()),
            (100000445, 2, "[]"),
            (100000445, 3, hourly.as_str()),
        ],
    );
    let stations = a.get_all_counting_stations().unwrap();
    assert_eq!(stations.len(), 2);

    let mut resolutions: HashMap<String, i64> = HashMap::new();
    let mut values: Vec<i64> = Vec::new();
    let mut calls = 0;
    loop {
        calls += 1;
        assert!(calls < 20, "source read must terminate");
        let batch = a.get_measurements_source(None, 1000).unwrap();
        for m in &batch.measurements {
            resolutions.insert(m.channel_external_id.clone(), m.record.resolution_seconds);
            values.push(m.record.value);
        }
        if !batch.more {
            break;
        }
    }
    assert_eq!(resolutions["100063085"], 900, "15-min station stays 15-min");
    assert_eq!(
        resolutions["100000445"], 3600,
        "hourly-only station falls back to hourly"
    );
    assert_eq!(values.len(), 6);
}

#[test]
fn filters_from_exclusive() {
    let rows = hourly_rows();
    let json = data_json(&rows);
    let a = adapter_with(
        &[("import_days_back", "10"), ("page_days", "30")],
        &[100063085],
        &[("publicwebpage/100063085", META_STEIN)],
        vec![(100063085, 2, "[]"), (100063085, 3, json.as_str())],
    );
    let (measurements, _) = read_all(&a, Some(rows[1].0));
    assert_eq!(measurements.len(), 1);
    assert_eq!(measurements[0].value, 12);
}

#[test]
fn multiple_stations_are_paged_to_completion() {
    let rows = hourly_rows();
    let json = data_json(&rows);
    let a = adapter_with(
        &[("import_days_back", "10"), ("page_days", "30")],
        &[100063085, 100000445],
        &[
            ("publicwebpage/100063085", META_STEIN),
            ("publicwebpage/100000445", META_EMPTY),
        ],
        vec![(100063085, 2, "[]"), (100063085, 3, json.as_str())],
    );
    let stations = a.get_all_counting_stations().unwrap();
    assert_eq!(stations.len(), 1);
    let (measurements, _) = read_all(&a, None);
    assert_eq!(measurements.len(), 3);
}

#[test]
fn health_is_down_for_an_unreachable_host() {
    let a = EcoCounterV1Adapter::with_fetcher(
        &config(&[("base_url", "http://127.0.0.1:1/")]),
        catalog(&[100063085]),
        Arc::new(FakeFetcher::new(vec![], vec![])),
    )
    .unwrap();
    assert!(matches!(a.check_health(), HealthStatus::Down(_)));
}

#[test]
fn parses_config_defaults() {
    let a = EcoCounterV1Adapter::with_fetcher(
        &config(&[]),
        catalog(&[100063085]),
        Arc::new(FakeFetcher::new(vec![], vec![])),
    )
    .unwrap();
    assert_eq!(a.cache_duration_secs(), 300);
}

#[test]
fn client_builds_day_window_url() {
    use super::client::{DEFAULT_BASE_URL, PublicWebpageClient};
    let c = PublicWebpageClient::new(
        DEFAULT_BASE_URL.to_string(),
        Arc::new(FakeFetcher::new(vec![], vec![])),
    );
    let url = c.data_url(
        100063085,
        7242,
        "tok",
        3,
        utc(2024, 6, 1, 0, 0, 0),
        utc(2024, 6, 8, 0, 0, 0),
    );
    assert!(url.contains("/pbl/publicwebpage/data/100063085?"));
    assert!(url.contains("begin=20240601"), "got {url}");
    assert!(url.contains("end=20240608"), "got {url}");
    assert!(url.contains("domain=7242"), "got {url}");
    assert!(url.contains("t=tok"), "got {url}");
}
