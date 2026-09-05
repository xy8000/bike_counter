//! Unit tests for the API_V1 mode provider (fixtures + fake fetcher, no network).

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

use super::catalog::CatalogStation;
use super::provider::EcoCounterV1Provider;

/// Reads the whole source through [`DataProvider::get_measurements_source`].
fn read_all(
    a: &EcoCounterV1Provider,
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

/// Serves fixture JSON keyed by a URL substring (data URLs matched first).
struct FakeFetcher {
    metadata: HashMap<String, String>,
    data: HashMap<String, String>,
}

impl FakeFetcher {
    fn new(metadata: Vec<(&str, &str)>, data: Vec<(&str, &str)>) -> Self {
        Self {
            metadata: metadata
                .into_iter()
                .map(|(k, v)| (k.to_string(), v.to_string()))
                .collect(),
            data: data
                .into_iter()
                .map(|(k, v)| (k.to_string(), v.to_string()))
                .collect(),
        }
    }
}

impl ResourceFetcher for FakeFetcher {
    fn fetch(&self, url: &str) -> Result<String, String> {
        if url.contains("/pbl/publicwebpage/data/") {
            for (key, json) in &self.data {
                if url.contains(key) {
                    return Ok(json.clone());
                }
            }
            return Err(format!("no fake data response for {url}"));
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
        DataProviderConfiguration::new("eco_counter_http_provider".to_string(), vars).unwrap();
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

/// Three hourly rows nine days ago (inside the tests' lookback window).
fn recent_rows() -> Vec<(DateTime<Utc>, i64)> {
    let now = Utc::now();
    let base = (now - Duration::days(9))
        .date_naive()
        .and_hms_opt(0, 0, 0)
        .map(|naive| DateTime::from_naive_utc_and_offset(naive, Utc))
        .expect("valid instant");
    vec![
        (base, 10),
        (base + Duration::hours(1), 11),
        (base + Duration::hours(2), 12),
    ]
}

fn adapter_with(
    vars: &[(&str, &str)],
    ids: &[i64],
    meta: &[(&str, &str)],
    data: &[(&str, &str)],
) -> EcoCounterV1Provider {
    EcoCounterV1Provider::with_fetcher(
        &config(vars),
        catalog(ids),
        Arc::new(FakeFetcher::new(meta.to_vec(), data.to_vec())),
    )
    .unwrap()
}

#[test]
fn discovers_station_and_channel_from_metadata() {
    let a = adapter_with(
        &[("v1_import_days_back", "10"), ("v1_page_days", "30")],
        &[100063085],
        &[("publicwebpage/100063085", META_STEIN)],
        &[],
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
        &[],
    );
    assert!(a.get_all_counting_stations().is_err());
}

#[test]
fn serves_the_cumulative_series_and_terminates() {
    let rows = recent_rows();
    let json = data_json(&rows);
    let a = adapter_with(
        &[
            ("v1_step", "3"),
            ("v1_import_days_back", "10"),
            ("v1_page_days", "30"),
        ],
        &[100063085],
        &[("publicwebpage/100063085", META_STEIN)],
        &[("publicwebpage/data/100063085", json.as_str())],
    );
    let (measurements, watermark) = read_all(&a, None);
    assert_eq!(measurements.len(), 3);
    assert_eq!(measurements[0].value, 10);
    assert_eq!(measurements[1].value, 11);
    assert_eq!(measurements[2].value, 12);
    assert_eq!(measurements[0].resolution_seconds, 3600);
    assert_eq!(watermark.expect("a watermark after a full read"), rows[2].0);
}

#[test]
fn filters_from_exclusive() {
    let rows = recent_rows();
    let json = data_json(&rows);
    let a = adapter_with(
        &[
            ("v1_step", "3"),
            ("v1_import_days_back", "10"),
            ("v1_page_days", "30"),
        ],
        &[100063085],
        &[("publicwebpage/100063085", META_STEIN)],
        &[("publicwebpage/data/100063085", json.as_str())],
    );
    let (measurements, _) = read_all(&a, Some(rows[1].0));
    assert_eq!(measurements.len(), 1);
    assert_eq!(measurements[0].value, 12);
}

#[test]
fn multiple_stations_are_paged_to_completion() {
    let rows_a = recent_rows();
    let json_a = data_json(&rows_a);
    let a = adapter_with(
        &[
            ("v1_step", "3"),
            ("v1_import_days_back", "10"),
            ("v1_page_days", "30"),
        ],
        &[100063085, 100000445],
        &[
            ("publicwebpage/100063085", META_STEIN),
            ("publicwebpage/100000445", META_EMPTY),
        ],
        &[("publicwebpage/data/100063085", json_a.as_str())],
    );
    let stations = a.get_all_counting_stations().unwrap();
    assert_eq!(stations.len(), 1);
    let (measurements, _) = read_all(&a, None);
    assert_eq!(measurements.len(), 3);
}

#[test]
fn health_is_down_for_an_unreachable_host() {
    let a = EcoCounterV1Provider::with_fetcher(
        &config(&[("v1_base_url", "http://127.0.0.1:1/")]),
        catalog(&[100063085]),
        Arc::new(FakeFetcher::new(vec![], vec![])),
    )
    .unwrap();
    assert!(matches!(a.check_health(), HealthStatus::Down(_)));
}

#[test]
fn parses_config_defaults() {
    let a = EcoCounterV1Provider::with_fetcher(
        &config(&[]),
        catalog(&[100063085]),
        Arc::new(FakeFetcher::new(vec![], vec![])),
    )
    .unwrap();
    assert_eq!(a.step(), 3);
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
