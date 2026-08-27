//! Unit tests for the Hamburg adapter (fixtures + fake fetcher, no network).

use std::collections::HashMap;
use std::sync::Arc;

use uuid::Uuid;

use crate::core::domain::channels::channel::Channel;
use crate::core::domain::channels::channel::value_objects as channel_vo;
use crate::core::domain::configuration::configuration::value_objects::{
    DataProviderConfiguration, DataSourceConfiguration,
};
use crate::core::domain::data_source::provider_port::{
    ChannelRecord, DataProvider, MeasurementQuery, ProviderError,
};
use crate::core::domain::health::HealthStatus;

use super::adapter::HamburgStaAdapter;
use super::fetcher::ResourceFetcher;
use super::parsing::utc;

/// A domain `Channel` whose external id is the `Zählfeld` id, as produced by the
/// import pipeline from the adapter's `ChannelRecord`.
fn field_channel(field_id: &str) -> Channel {
    Channel {
        id: channel_vo::Id(Uuid::new_v4()),
        counting_station_id: channel_vo::CountingStationId(Uuid::new_v4()),
        name: channel_vo::Name(field_id.to_string()),
        description: channel_vo::Description(String::new()),
        external_datasource_id: Some(channel_vo::ExternalDatasourceId(field_id.to_string())),
    }
}

/// Serves fixture JSON keyed by a URL substring.
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
        for (key, json) in &self.responses {
            if url.contains(key) {
                return Ok(json.clone());
            }
        }
        Err(format!("no fake response for {url}"))
    }
}

const DISCOVERY: &str = r#"{"value":[
 {"@iot.id":26394,"name":"Rad-Aufkommen an Verkehrszählfeld B_11.1_1_G im 5-Min-Intervall am MQ11.1",
  "properties":{"assetID":"B_11.1_1_G","knotenName":"MQ11.1","layerName":"Anzahl_Fahrraeder_Zaehlfeld_5-Min"},
  "observedArea":{"type":"Point","coordinates":[10.0276723,53.5332298]},
  "Thing":{"properties":{"richtung":"Richtung 1"}}},
 {"@iot.id":26140,"name":"Fahrradaufkommen an Zählfeld B_11.1_1_G im 5-Min-Intervall (veraltet)",
  "properties":{"layerName":"Anzahl_Fahrraeder_Zaehlfeld_5-Min"}},
 {"@iot.id":26400,"name":"Rad-Aufkommen an Verkehrszählfeld B_11.1_2_I im 5-Min-Intervall am MQ11.1",
  "properties":{"assetID":"B_11.1_2_I","knotenName":"MQ11.1"},
  "observedArea":{"type":"Point","coordinates":[10.0277,53.5333]},
  "Thing":{"properties":{"richtung":"Richtung 2"}}}
]}"#;

/// Legacy observations for field B_11.1_1_G (datastream 26140): one row plus a
/// sentinel that must be skipped.
const LEGACY_OBS: &str = r#"{"value":[
 {"phenomenonTime":"1990-06-01T00:00:00Z","result":0},
 {"phenomenonTime":"2026-03-01T00:00:00Z/2026-03-01T00:04:59Z","result":1},
 {"phenomenonTime":"2026-03-01T00:05:00Z/2026-03-01T00:09:59Z","result":2}
]}"#;

/// Current observations for field B_11.1_1_G (datastream 26394): overlaps the
/// legacy row at 2026-03-01T00:00 (current wins) and continues after.
const CURRENT_OBS: &str = r#"{"value":[
 {"phenomenonTime":"2026-03-01T00:00:00Z/2026-03-01T00:04:59Z","result":9},
 {"phenomenonTime":"2026-03-01T00:05:00Z/2026-03-01T00:09:59Z","result":10},
 {"phenomenonTime":"2026-03-01T00:10:00Z/2026-03-01T00:14:59Z","result":11}
]}"#;

/// Current observations for field B_11.1_2_I (datastream 26400): four rows so
/// that filtering `from` = 00:00 exclusively leaves three (00:05, 00:10, 00:15),
/// i.e. more than the batch cap of two, which must raise the page-again flag.
const FIELD2_OBS: &str = r#"{"value":[
 {"phenomenonTime":"2026-03-01T00:00:00Z/2026-03-01T00:04:59Z","result":1},
 {"phenomenonTime":"2026-03-01T00:05:00Z/2026-03-01T00:09:59Z","result":2},
 {"phenomenonTime":"2026-03-01T00:10:00Z/2026-03-01T00:14:59Z","result":3},
 {"phenomenonTime":"2026-03-01T00:15:00Z/2026-03-01T00:19:59Z","result":4}
]}"#;

fn config(vars: &[(&str, &str)]) -> DataSourceConfiguration {
    let vars: HashMap<String, String> = vars
        .iter()
        .map(|(k, v)| (k.to_string(), v.to_string()))
        .collect();
    let provider =
        DataProviderConfiguration::new(HamburgStaAdapter::provider_type().to_string(), vars)
            .unwrap();
    DataSourceConfiguration::new("Hamburg".to_string(), provider).unwrap()
}

fn adapter(responses: Vec<(&str, &str)>) -> HamburgStaAdapter {
    HamburgStaAdapter::with_fetcher(&config(&[]), Arc::new(FakeFetcher::new(responses))).unwrap()
}

#[test]
fn parses_config_defaults() {
    let adapter = HamburgStaAdapter::new(&config(&[])).unwrap();
    assert_eq!(adapter.cache_duration_secs(), 300);
}

#[test]
fn parses_custom_config() {
    let adapter = HamburgStaAdapter::new(&config(&[
        ("base_url", "https://iot.hamburg.de/v1.0"),
        ("cache_duration", "600"),
        ("include_legacy", "false"),
    ]))
    .unwrap();
    assert_eq!(adapter.cache_duration_secs(), 600);
}

#[test]
fn rejects_invalid_config() {
    assert!(HamburgStaAdapter::new(&config(&[("cache_duration", "nope")])).is_err());
}

#[test]
fn discovers_mq_stations_and_field_channels() {
    let a = adapter(vec![("Datastreams?", DISCOVERY)]);

    let stations = a.get_all_counting_stations().unwrap();
    assert_eq!(stations.len(), 1);
    assert_eq!(stations[0].external_id, "MQ11.1");
    assert_eq!(stations[0].name, "MQ11.1");
    assert_eq!(stations[0].latitude, Some(53.5332298));
    assert_eq!(stations[0].longitude, Some(10.0276723));
    assert_eq!(stations[0].timezone, "Europe/Berlin");

    let channels: Vec<ChannelRecord> = a.get_all_channels().unwrap();
    assert_eq!(channels.len(), 2);
    assert!(channels.iter().any(|c| {
        c.external_id == "B_11.1_1_G"
            && c.name == "B_11.1_1_G (Richtung 1)"
            && c.counting_station_external_id == "MQ11.1"
    }));
}

#[test]
fn serves_merged_measurements_with_current_winning_on_overlap() {
    let a = adapter(vec![
        ("Datastreams?", DISCOVERY),
        ("Datastreams(26140)/Observations", LEGACY_OBS),
        ("Datastreams(26394)/Observations", CURRENT_OBS),
    ]);

    // from = one microsecond before the first row -> everything is included.
    let from = utc(2026, 3, 1, 0, 0, 0) - chrono::Duration::microseconds(1);
    let batch = a
        .get_measurements(
            MeasurementQuery::for_channel(field_channel("B_11.1_1_G"), 100).with_start(from),
        )
        .unwrap();

    // 3 current rows + 1 legacy row (the sentinel + the overlapping legacy row
    // are gone): legacy 00:05 = 2 is superseded by current 00:05 = 10.
    let rows = &batch.measurements;
    assert_eq!(rows.len(), 3);
    assert_eq!(rows[0].timestamp, utc(2026, 3, 1, 0, 0, 0));
    assert_eq!(rows[0].value, 9, "the current value wins on the overlap");
    assert_eq!(rows[0].resolution_seconds, 300);
    assert_eq!(rows[1].value, 10);
    assert_eq!(rows[2].value, 11);
    assert!(!batch.batch_size_limit_reached);
}

#[test]
fn filters_from_exclusive_and_truncates_to_batch_size() {
    let a = adapter(vec![
        ("Datastreams?", DISCOVERY),
        ("Datastreams(26400)/Observations", FIELD2_OBS),
    ]);

    // From after the first row -> only rows strictly after `from`.
    let from = utc(2026, 3, 1, 0, 0, 0);
    let batch = a
        .get_measurements(
            MeasurementQuery::for_channel(field_channel("B_11.1_2_I"), 2).with_start(from),
        )
        .unwrap();
    assert_eq!(batch.measurements.len(), 2);
    assert_eq!(batch.measurements[0].timestamp, utc(2026, 3, 1, 0, 5, 0));
    assert!(
        batch.batch_size_limit_reached,
        "a small batch must page again"
    );
    assert_eq!(
        batch.last_measurement_datetime,
        Some(utc(2026, 3, 1, 0, 10, 0))
    );
}

#[test]
fn unknown_field_is_invalid_data() {
    let a = adapter(vec![("Datastreams?", DISCOVERY)]);
    let result = a.get_measurements(MeasurementQuery::for_channel(field_channel("NOPE"), 100));
    assert!(matches!(result, Err(ProviderError::InvalidData(_))));
}

#[test]
fn health_is_down_for_an_unreachable_host() {
    let a = HamburgStaAdapter::with_fetcher(
        &config(&[("base_url", "http://localhost:1/")]),
        Arc::new(FakeFetcher::new(vec![])),
    )
    .unwrap();
    assert!(matches!(a.check_health(), HealthStatus::Down(_)));
}
