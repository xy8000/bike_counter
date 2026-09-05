//! Unit tests for the API_V2 mode provider (fixtures + fake fetcher, no network).

use std::collections::HashMap;
use std::sync::Arc;

use chrono::{DateTime, Duration, Utc};

use crate::adapter::driven::eco_counter::fetcher::ResourceFetcher;
use crate::core::domain::configuration::configuration::value_objects::{
    DataProviderConfiguration, DataSourceConfiguration,
};
use crate::core::domain::data_source::provider_port::{
    ChannelRecord, DataProvider, MeasurementRecord,
};
use crate::core::domain::health::HealthStatus;

use super::provider::EcoCounterV2Provider;

const SITES: &str = r#"[
  {"id":7,"name":"Site A","domainId":118,"domain":"Demo Org","latitude":49.0,"longitude":8.0,
   "timezone":"(UTC+01:00) Europe/Berlin;DST","interval":60,"sens":2},
  {"id":9,"name":"Site B","domainId":118,"domain":"Demo Org","latitude":50.0,"longitude":9.0,
   "timezone":"(UTC+01:00) Europe/Berlin;DST","interval":60}
]"#;

fn read_all(
    a: &EcoCounterV2Provider,
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

struct FakeFetcher {
    responses: HashMap<String, String>,
}

impl FakeFetcher {
    fn new(responses: Vec<(&str, &str)>) -> Self {
        Self {
            responses: responses
                .into_iter()
                .map(|(k, v)| (k.to_string(), v.to_string()))
                .collect(),
        }
    }
}

impl ResourceFetcher for FakeFetcher {
    fn fetch(&self, url: &str) -> Result<String, String> {
        // Data URLs contain "/data/site/"; discovery URLs contain "/site".
        let is_data = url.contains("/data/site/");
        for (key, json) in &self.responses {
            if is_data && key.contains("/data/") && url.contains(key) {
                return Ok(json.clone());
            }
        }
        if !is_data {
            for (key, json) in &self.responses {
                if !key.contains("/data/") && url.contains(key) {
                    return Ok(json.clone());
                }
            }
        }
        Err(format!("no fake response for {url}"))
    }
}

fn config(vars: &[(&str, &str)]) -> DataSourceConfiguration {
    let mut vars: HashMap<String, String> = vars
        .iter()
        .map(|(k, v)| (k.to_string(), v.to_string()))
        .collect();
    vars.entry("v2_access_token".to_string())
        .or_insert_with(|| "tok".to_string());
    let provider =
        DataProviderConfiguration::new("eco_counter_http_provider".to_string(), vars).unwrap();
    DataSourceConfiguration::new("Eco-Counter V2".to_string(), provider).unwrap()
}

fn adapter(vars: &[(&str, &str)], responses: Vec<(&str, &str)>) -> EcoCounterV2Provider {
    EcoCounterV2Provider::with_fetcher(&config(vars), Arc::new(FakeFetcher::new(responses)))
        .unwrap()
}

fn points_json(rows: &[(DateTime<Utc>, i64)]) -> String {
    let items: Vec<String> = rows
        .iter()
        .map(|(t, value)| {
            format!(
                "{{\"date\":\"{}\",\"isoDate\":\"{}\",\"counts\":{}}}",
                t.format("%Y-%m-%dT%H:%M:%S%z"),
                t.format("%Y-%m-%dT%H:%M:%S%z"),
                value
            )
        })
        .collect();
    format!("[{}]", items.join(","))
}

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

#[test]
fn discovers_sites_as_stations_with_one_channel_each() {
    let a = adapter(
        &[("v2_import_days_back", "10"), ("v2_page_days", "30")],
        vec![("/site", SITES)],
    );
    let stations = a.get_all_counting_stations().unwrap();
    assert_eq!(stations.len(), 2);
    assert_eq!(stations[0].external_id, "7");
    assert_eq!(stations[0].name, "Site A");
    assert_eq!(stations[0].timezone, "Europe/Berlin");
    let channels: Vec<ChannelRecord> = a.get_all_channels().unwrap();
    assert_eq!(channels.len(), 2);
    assert!(channels.iter().any(|c| c.external_id == "9"));
}

#[test]
fn serves_site_series_and_terminates() {
    let rows = recent_rows();
    let json = points_json(&rows);
    let a = adapter(
        &[
            ("v2_step", "3"),
            ("v2_import_days_back", "10"),
            ("v2_page_days", "30"),
        ],
        vec![
            ("/site", SITES),
            ("/data/site/7", json.as_str()),
            ("/data/site/9", "[]"),
        ],
    );
    let (measurements, watermark) = read_all(&a, None);
    assert_eq!(measurements.len(), 3);
    assert_eq!(measurements[0].value, 10);
    assert_eq!(measurements[0].resolution_seconds, 3600);
    assert_eq!(watermark.expect("a watermark after a full read"), rows[2].0);
}

#[test]
fn rejects_missing_access_token() {
    let vars = HashMap::new();
    let provider =
        DataProviderConfiguration::new("eco_counter_http_provider".to_string(), vars).unwrap();
    let config = DataSourceConfiguration::new("Eco-Counter V2".to_string(), provider).unwrap();
    assert!(EcoCounterV2Provider::new(&config).is_err());
}

#[test]
fn health_is_down_for_an_unreachable_host() {
    let a = EcoCounterV2Provider::with_fetcher(
        &config(&[("v2_base_url", "http://127.0.0.1:1/")]),
        Arc::new(FakeFetcher::new(vec![])),
    )
    .unwrap();
    assert!(matches!(a.check_health(), HealthStatus::Down(_)));
}

#[test]
fn parses_config_defaults() {
    let a = EcoCounterV2Provider::with_fetcher(&config(&[]), Arc::new(FakeFetcher::new(vec![])))
        .unwrap();
    assert_eq!(a.step(), 3);
    assert_eq!(a.cache_duration_secs(), 300);
}

#[test]
fn filters_from_exclusive() {
    let rows = recent_rows();
    let json = points_json(&rows);
    let a = adapter(
        &[
            ("v2_step", "3"),
            ("v2_import_days_back", "10"),
            ("v2_page_days", "30"),
        ],
        vec![
            ("/site", SITES),
            ("/data/site/7", json.as_str()),
            ("/data/site/9", "[]"),
        ],
    );
    let (measurements, _) = read_all(&a, Some(rows[1].0));
    assert_eq!(measurements.len(), 1);
    assert_eq!(measurements[0].value, 12);
}
