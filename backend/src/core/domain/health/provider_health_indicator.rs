//! Bridges a [`DataProvider`] into the health system so each configured data
//! source appears in the readiness report.

use std::sync::Arc;

use crate::core::domain::data_source::provider_port::DataProvider;
use crate::core::domain::health::{HealthStatus, ServiceHealthIndicator};

/// A [`ServiceHealthIndicator`] backed by a single data provider.
pub struct ProviderHealthIndicator {
    name: String,
    provider: Arc<dyn DataProvider>,
}

impl ProviderHealthIndicator {
    pub fn new(name: String, provider: Arc<dyn DataProvider>) -> Self {
        Self { name, provider }
    }
}

impl ServiceHealthIndicator for ProviderHealthIndicator {
    fn name(&self) -> String {
        self.name.clone()
    }

    fn check(&self) -> HealthStatus {
        self.provider.check_health()
    }
}

#[cfg(test)]
mod tests {
    use std::sync::Arc;

    use super::ProviderHealthIndicator;
    use crate::core::domain::data_source::provider_port::DataProvider;
    use crate::core::domain::data_source::provider_port::ProviderError;
    use crate::core::domain::data_source::provider_port::SourceMeasurementBatch;
    use crate::core::domain::health::{HealthStatus, ServiceHealthIndicator};

    struct MockProvider {
        status: HealthStatus,
    }

    impl DataProvider for MockProvider {
        fn check_health(&self) -> HealthStatus {
            self.status.clone()
        }

        fn get_all_counting_stations(
            &self,
        ) -> Result<
            Vec<crate::core::domain::data_source::provider_port::CountingStationRecord>,
            ProviderError,
        > {
            Ok(vec![])
        }

        fn get_all_channels(
            &self,
        ) -> Result<
            Vec<crate::core::domain::data_source::provider_port::ChannelRecord>,
            ProviderError,
        > {
            Ok(vec![])
        }

        fn get_measurements_source(
            &self,
            _from: Option<chrono::DateTime<chrono::Utc>>,
            _max_batch_size: usize,
        ) -> Result<SourceMeasurementBatch, ProviderError> {
            Ok(SourceMeasurementBatch {
                measurements: vec![],
                next_from: None,
                more: false,
            })
        }

        fn max_measurement_batch_size(&self) -> usize {
            1
        }
    }

    #[test]
    fn reports_dynamic_name_and_provider_status() {
        let indicator = ProviderHealthIndicator::new(
            "Münster/münster_opendata_github_provider".to_string(),
            Arc::new(MockProvider {
                status: HealthStatus::Down("unreachable".to_string()),
            }),
        );

        assert_eq!(indicator.name(), "Münster/münster_opendata_github_provider");
        assert_eq!(
            indicator.check(),
            HealthStatus::Down("unreachable".to_string())
        );
    }
}
