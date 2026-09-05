//! Unit tests for the ScreenScraping scaffold (no network).

use std::collections::HashMap;
use std::sync::Arc;

use crate::adapter::driven::eco_counter::fetcher::{HttpResourceFetcher, ResourceFetcher};
use crate::core::domain::configuration::configuration::value_objects::{
    DataProviderConfiguration, DataSourceConfiguration,
};
use crate::core::domain::data_source::provider_port::DataProvider;
use crate::core::domain::health::HealthStatus;

use super::provider::EcoCounterScreenScrapingProvider;

fn config(vars: &[(&str, &str)]) -> DataSourceConfiguration {
    let vars: HashMap<String, String> = vars
        .iter()
        .map(|(k, v)| (k.to_string(), v.to_string()))
        .collect();
    let provider =
        DataProviderConfiguration::new("eco_counter_http_provider".to_string(), vars).unwrap();
    DataSourceConfiguration::new("Eco-Counter Web".to_string(), provider).unwrap()
}

#[test]
fn serves_no_stations_yet() {
    let a = EcoCounterScreenScrapingProvider::with_fetcher(
        &config(&[]),
        Arc::new(HttpResourceFetcher::new()),
    )
    .unwrap();
    assert!(a.get_all_counting_stations().unwrap().is_empty());
    assert!(a.get_all_channels().unwrap().is_empty());
    let batch = a.get_measurements_source(None, 100).unwrap();
    assert!(batch.measurements.is_empty());
    assert!(!batch.more);
}

#[test]
fn health_reflects_scrape_url_host() {
    let a = EcoCounterScreenScrapingProvider::with_fetcher(
        &config(&[("web_scrape_url", "http://127.0.0.1:1/")]),
        Arc::new(HttpResourceFetcher::new()),
    )
    .unwrap();
    assert!(matches!(a.check_health(), HealthStatus::Down(_)));
}

struct StubFetcher;
impl ResourceFetcher for StubFetcher {
    fn fetch(&self, url: &str) -> Result<String, String> {
        Err(format!("unexpected fetch: {url}"))
    }
}

#[test]
fn rejects_empty_scrape_url() {
    let r = EcoCounterScreenScrapingProvider::with_fetcher(
        &config(&[("web_scrape_url", "")]),
        Arc::new(StubFetcher),
    );
    assert!(r.is_err());
}
