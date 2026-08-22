//! Decides what should happen at startup in the data-source domain:
//! read the configuration, build a provider per data source, sync the persisted
//! data sources (add new / remove stale) and prepare their health indicators.
//! `main.rs` only wires dependencies and calls [`StartupService::run`].

use std::collections::HashSet;
use std::sync::Arc;

use crate::core::application::data_import_service::DataSourceRuntime;
use crate::core::application::data_provider_factory::DataProviderFactory;
use crate::core::domain::configuration::error::ConfigError;
use crate::core::domain::configuration::repository::ConfigurationRepository;
use crate::core::domain::data_source::data_source::DataSource;
use crate::core::domain::data_source::data_source::value_objects::Id as DataSourceId;
use crate::core::domain::data_source::health_indicator::ProviderHealthIndicator;
use crate::core::domain::data_source::repository::DataSourceRepository;
use crate::core::domain::error::DomainError;
use crate::core::domain::health::ServiceHealthIndicator;

/// The artifacts produced by a successful startup run.
pub struct StartupResult {
    /// One runtime per configured data source (for the deferred import feature).
    pub data_source_runtimes: Vec<DataSourceRuntime>,
    /// A health indicator per configured data source.
    pub provider_health_indicators: Vec<Arc<dyn ServiceHealthIndicator>>,
}

/// A failure during startup orchestration.
#[derive(Debug)]
pub enum StartupError {
    /// The configuration itself is wrong (blocks startup).
    Configuration(ConfigError),
    /// Persisting the synced data sources failed.
    Database(DomainError),
}

impl From<ConfigError> for StartupError {
    fn from(error: ConfigError) -> Self {
        StartupError::Configuration(error)
    }
}

impl From<DomainError> for StartupError {
    fn from(error: DomainError) -> Self {
        StartupError::Database(error)
    }
}

pub struct StartupService {
    configuration_repository: Arc<dyn ConfigurationRepository + Send + Sync>,
    data_source_repository: Arc<dyn DataSourceRepository + Send + Sync>,
    data_provider_factory: Arc<dyn DataProviderFactory>,
}

impl StartupService {
    pub fn new(
        configuration_repository: Arc<dyn ConfigurationRepository + Send + Sync>,
        data_source_repository: Arc<dyn DataSourceRepository + Send + Sync>,
        data_provider_factory: Arc<dyn DataProviderFactory>,
    ) -> Self {
        Self {
            configuration_repository,
            data_source_repository,
            data_provider_factory,
        }
    }

    pub fn run(&self) -> Result<StartupResult, StartupError> {
        let configuration = self.configuration_repository.read_configuration()?;

        let mut runtimes = Vec::new();
        let mut indicators = Vec::new();
        let mut configured_ids = HashSet::new();

        for data_source in configuration.data_sources() {
            let provider = self.data_provider_factory.build(data_source)?;
            let data_source_id = DataSourceId(DataSource::id_from_name(data_source.name()));
            configured_ids.insert(data_source_id);

            // Persist the configured data source (id is deterministic from name).
            self.data_source_repository.upsert(DataSource::new(
                data_source.name().to_string(),
                data_source.provider().provider_type().to_string(),
            ))?;

            let name = format!(
                "{}/{}",
                data_source.name(),
                data_source.provider().provider_type()
            );
            indicators.push(
                Arc::new(ProviderHealthIndicator::new(name, provider.clone()))
                    as Arc<dyn ServiceHealthIndicator>,
            );

            runtimes.push(DataSourceRuntime {
                configuration: data_source.clone(),
                data_source_id,
                provider,
            });
        }

        // Remove persisted data sources that are no longer configured. The FK on
        // counting_stations uses ON DELETE SET NULL, so no data is lost.
        for existing in self.data_source_repository.find_all()? {
            if !configured_ids.contains(&existing.id) {
                self.data_source_repository.delete(existing.id)?;
            }
        }

        Ok(StartupResult {
            data_source_runtimes: runtimes,
            provider_health_indicators: indicators,
        })
    }
}

#[cfg(test)]
mod tests {
    use std::collections::HashMap;
    use std::sync::{Arc, Mutex};

    use chrono::{DateTime, Utc};

    use super::*;
    use crate::core::domain::channels::channel::Channel;
    use crate::core::domain::configuration::configuration::value_objects::{
        DataProviderConfiguration, DataSourceConfiguration, DatabaseConfiguration,
    };
    use crate::core::domain::configuration::configuration::{
        Configuration, DEFAULT_DATA_SOURCE_UPDATE_CRON,
    };
    use crate::core::domain::counting_stations::counting_station::CountingStation;
    use crate::core::domain::data_source::provider::{
        DataProvider, MeasurementBatch, MeasurementQuery, ProviderError,
    };
    use crate::core::domain::health::HealthStatus;

    fn database() -> DatabaseConfiguration {
        DatabaseConfiguration::new(
            "postgres://localhost:5432".to_string(),
            "user".to_string(),
            "password".to_string(),
            "database".to_string(),
        )
        .unwrap()
    }

    fn data_source_config(name: &str, provider_type: &str) -> DataSourceConfiguration {
        DataSourceConfiguration::new(
            name.to_string(),
            DataProviderConfiguration::new(provider_type.to_string(), HashMap::new()).unwrap(),
        )
        .unwrap()
    }

    fn configuration(names: &[&str]) -> Configuration {
        Configuration::new(
            database(),
            names
                .iter()
                .map(|name| data_source_config(name, "münster_opendata_github_provider"))
                .collect(),
            DEFAULT_DATA_SOURCE_UPDATE_CRON.to_string(),
            3600,
        )
        .unwrap()
    }

    struct MockConfigurationRepository {
        configuration: Configuration,
    }

    impl ConfigurationRepository for MockConfigurationRepository {
        fn read_configuration(&self) -> Result<Configuration, ConfigError> {
            Ok(self.configuration.clone())
        }
    }

    /// In-memory data-source repository; `fail_upsert` simulates database errors.
    struct MockDataSourceRepository {
        data_sources: Mutex<Vec<DataSource>>,
        fail_upsert: bool,
    }

    impl MockDataSourceRepository {
        fn new(initial: Vec<DataSource>) -> Self {
            Self {
                data_sources: Mutex::new(initial),
                fail_upsert: false,
            }
        }
    }

    impl DataSourceRepository for MockDataSourceRepository {
        fn upsert(&self, data_source: DataSource) -> Result<(), DomainError> {
            if self.fail_upsert {
                return Err(DomainError::Database("upsert failed".to_string()));
            }
            let mut data_sources = self.data_sources.lock().unwrap();
            if let Some(existing) = data_sources.iter_mut().find(|ds| ds.id == data_source.id) {
                *existing = data_source;
            } else {
                data_sources.push(data_source);
            }
            Ok(())
        }

        fn find_by_id(&self, id: DataSourceId) -> Result<Option<DataSource>, DomainError> {
            Ok(self
                .data_sources
                .lock()
                .unwrap()
                .iter()
                .find(|ds| ds.id == id)
                .cloned())
        }

        fn find_by_name(&self, name: &str) -> Result<Option<DataSource>, DomainError> {
            Ok(self
                .data_sources
                .lock()
                .unwrap()
                .iter()
                .find(|ds| ds.name.0 == name)
                .cloned())
        }

        fn find_all(&self) -> Result<Vec<DataSource>, DomainError> {
            Ok(self.data_sources.lock().unwrap().clone())
        }

        fn delete(&self, id: DataSourceId) -> Result<(), DomainError> {
            self.data_sources.lock().unwrap().retain(|ds| ds.id != id);
            Ok(())
        }

        fn update_last_updated_at(
            &self,
            id: DataSourceId,
            timestamp: DateTime<Utc>,
        ) -> Result<(), DomainError> {
            if let Some(data_source) = self
                .data_sources
                .lock()
                .unwrap()
                .iter_mut()
                .find(|ds| ds.id == id)
            {
                data_source.last_updated_at = Some(timestamp);
            }
            Ok(())
        }
    }

    /// A minimal provider; the data-serving methods are never reached in these tests.
    struct MockProvider;

    impl DataProvider for MockProvider {
        fn check_health(&self) -> HealthStatus {
            HealthStatus::Up
        }

        fn get_all_counting_stations(&self) -> Result<Vec<CountingStation>, ProviderError> {
            Ok(Vec::new())
        }

        fn get_all_channels(&self) -> Result<Vec<Channel>, ProviderError> {
            Ok(Vec::new())
        }

        fn get_measurements(
            &self,
            _query: MeasurementQuery,
        ) -> Result<MeasurementBatch, ProviderError> {
            Ok(MeasurementBatch {
                measurements: Vec::new(),
                last_measurement_datetime: None,
                batch_size_limit_reached: false,
            })
        }

        fn max_measurement_batch_size(&self) -> usize {
            500
        }
    }

    struct MockDataProviderFactory;

    impl DataProviderFactory for MockDataProviderFactory {
        fn build(
            &self,
            _config: &DataSourceConfiguration,
        ) -> Result<Arc<dyn DataProvider>, ConfigError> {
            Ok(Arc::new(MockProvider))
        }
    }

    fn service(
        config_repo: MockConfigurationRepository,
        data_source_repo: Arc<MockDataSourceRepository>,
    ) -> StartupService {
        StartupService::new(
            Arc::new(config_repo),
            data_source_repo,
            Arc::new(MockDataProviderFactory),
        )
    }

    #[test]
    fn upserts_configured_data_sources_and_builds_indicators() {
        let data_source_repo = Arc::new(MockDataSourceRepository::new(Vec::new()));
        let startup_service = service(
            MockConfigurationRepository {
                configuration: configuration(&["Münster"]),
            },
            data_source_repo.clone(),
        );

        let result = startup_service.run().expect("startup should succeed");

        assert_eq!(result.data_source_runtimes.len(), 1);
        assert_eq!(result.provider_health_indicators.len(), 1);
        assert_eq!(
            result.provider_health_indicators[0].name(),
            "Münster/münster_opendata_github_provider"
        );

        let persisted = data_source_repo.data_sources.lock().unwrap();
        assert_eq!(persisted.len(), 1);
        assert_eq!(persisted[0].name.0, "Münster");
        assert_eq!(
            persisted[0].provider_type.0,
            "münster_opendata_github_provider"
        );
    }

    #[test]
    fn removes_data_sources_that_are_no_longer_configured() {
        let stale = DataSource::new("Old".to_string(), "old_provider".to_string());
        let data_source_repo = Arc::new(MockDataSourceRepository::new(vec![stale]));
        let startup_service = service(
            MockConfigurationRepository {
                configuration: configuration(&[]),
            },
            data_source_repo.clone(),
        );

        let result = startup_service.run().expect("startup should succeed");

        assert!(result.data_source_runtimes.is_empty());
        assert!(result.provider_health_indicators.is_empty());
        assert!(
            data_source_repo.data_sources.lock().unwrap().is_empty(),
            "stale data source should have been deleted"
        );
    }

    #[test]
    fn removes_only_stale_data_sources() {
        let configured = DataSource::new("Münster".to_string(), "p".to_string());
        let stale = DataSource::new("Old".to_string(), "p".to_string());
        let data_source_repo = Arc::new(MockDataSourceRepository::new(vec![configured, stale]));
        let startup_service = service(
            MockConfigurationRepository {
                configuration: configuration(&["Münster"]),
            },
            data_source_repo.clone(),
        );

        startup_service.run().expect("startup should succeed");

        let persisted = data_source_repo.data_sources.lock().unwrap();
        assert_eq!(persisted.len(), 1);
        assert_eq!(persisted[0].name.0, "Münster");
    }

    #[test]
    fn propagates_database_errors() {
        let data_source_repo = Arc::new(MockDataSourceRepository {
            data_sources: Mutex::new(Vec::new()),
            fail_upsert: true,
        });
        let startup_service = service(
            MockConfigurationRepository {
                configuration: configuration(&["Münster"]),
            },
            data_source_repo,
        );

        assert!(matches!(
            startup_service.run(),
            Err(StartupError::Database(DomainError::Database(_)))
        ));
    }
}
