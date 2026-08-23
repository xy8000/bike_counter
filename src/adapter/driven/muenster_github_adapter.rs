//! Data provider for the Münster open-data GitHub archive.
//!
//! Baseline: configuration parsing and a real reachability health check are
//! implemented. The GitHub download / CSV parsing for stations, channels and
//! measurements is a follow-up task; the data-serving methods return empty data.

use std::net::TcpStream;
use std::sync::{Arc, Mutex};

use chrono::Utc;

use crate::core::domain::channels::channel::Channel;
use crate::core::domain::configuration::configuration::value_objects::DataSourceConfiguration;
use crate::core::domain::configuration::error::ConfigError;
use crate::core::domain::counting_stations::counting_station::CountingStation;
use crate::core::domain::data_source::provider::{
    DataProvider, MeasurementBatch, MeasurementQuery, PersistentStateAccess, ProviderError,
};
use crate::core::domain::health::HealthStatus;

const PROVIDER_TYPE: &str = "münster_opendata_github_provider";
const DEFAULT_MAX_MEASUREMENT_BATCH_SIZE: usize = 500;
const DEFAULT_CACHE_DURATION_SECS: u64 = 300;

// The batch-size getter is only consumed by the deferred import feature.
pub struct MuensterGithubAdapter {
    url: String,
    max_measurement_batch_size: usize,
    /// Cache window in seconds for the (follow-up) archive cache; default 300.
    cache_duration: u64,
    /// Scoped persistent-state handle, attached by `StartupService` after the
    /// data source is persisted (two-phase handover). `None` until attached.
    state: Mutex<Option<Arc<dyn PersistentStateAccess + Send + Sync>>>,
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
    /// `cache_duration` (seconds, default `300`). A missing/invalid value is a
    /// configuration error (blocks startup).
    pub fn new(config: &DataSourceConfiguration) -> Result<Self, ConfigError> {
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

        Ok(Self {
            url,
            max_measurement_batch_size,
            cache_duration,
            state: Mutex::new(None),
        })
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

    fn get_all_counting_stations(&self) -> Result<Vec<CountingStation>, ProviderError> {
        // TODO: download the GitHub archive and parse the counting stations CSV.
        Ok(Vec::new())
    }

    fn get_all_channels(&self) -> Result<Vec<Channel>, ProviderError> {
        // TODO: download the GitHub archive and parse the channels CSV.
        Ok(Vec::new())
    }

    fn get_measurements(
        &self,
        _query: MeasurementQuery,
    ) -> Result<MeasurementBatch, ProviderError> {
        // TODO: download the GitHub archive and parse the measurements for the
        // requested channel, bounded by query.max_batch_size.
        let _ = Utc::now();
        Ok(MeasurementBatch {
            measurements: Vec::new(),
            last_measurement_datetime: None,
            batch_size_limit_reached: false,
        })
    }

    fn max_measurement_batch_size(&self) -> usize {
        self.max_measurement_batch_size
    }

    fn attach_persistent_state(&self, state: Arc<dyn PersistentStateAccess + Send + Sync>) {
        *self.state.lock().unwrap() = Some(state);
    }
}

impl MuensterGithubAdapter {
    /// The configured cache window in seconds (used by the follow-up archive
    /// cache plan).
    pub fn cache_duration(&self) -> u64 {
        self.cache_duration
    }

    /// The attached persistent-state handle, if any (test-only accessor).
    #[cfg(test)]
    fn attached_state(&self) -> Option<Arc<dyn PersistentStateAccess + Send + Sync>> {
        self.state.lock().unwrap().clone()
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
    use std::sync::{Arc, Mutex};

    use super::{MuensterGithubAdapter, parse_host_and_port};
    use crate::core::domain::configuration::configuration::value_objects::{
        DataProviderConfiguration, DataSourceConfiguration,
    };
    use crate::core::domain::configuration::error::ConfigError;
    use crate::core::domain::data_source::provider::{
        DataProvider, PersistentStateAccess, ProviderError,
    };
    use crate::core::domain::health::HealthStatus;

    fn data_source(vars: HashMap<String, String>) -> DataSourceConfiguration {
        let provider = DataProviderConfiguration::new(
            MuensterGithubAdapter::provider_type().to_string(),
            vars,
        )
        .unwrap();
        DataSourceConfiguration::new("Münster".to_string(), provider).unwrap()
    }

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
        // A closed port with no listener refuses the connection immediately.
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
}
