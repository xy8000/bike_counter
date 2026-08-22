//! Bridges a [`DataProvider`] into the health system so each configured data
//! source appears in the readiness report.

use std::sync::Arc;

use crate::core::domain::data_source::provider::DataProvider;
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
    use crate::core::domain::channels::channel::Channel;
    use crate::core::domain::counting_stations::counting_station::CountingStation;
    use crate::core::domain::data_source::provider::DataProvider;
    use crate::core::domain::data_source::provider::MeasurementBatch;
    use crate::core::domain::data_source::provider::MeasurementQuery;
    use crate::core::domain::data_source::provider::ProviderError;
    use crate::core::domain::health::{HealthStatus, ServiceHealthIndicator};

    struct MockProvider {
        status: HealthStatus,
    }

    impl DataProvider for MockProvider {
        fn check_health(&self) -> HealthStatus {
            self.status.clone()
        }

        fn get_all_counting_stations(&self) -> Result<Vec<CountingStation>, ProviderError> {
            Ok(vec![])
        }

        fn get_all_channels(&self) -> Result<Vec<Channel>, ProviderError> {
            Ok(vec![])
        }

        fn get_measurements(
            &self,
            _query: MeasurementQuery,
        ) -> Result<MeasurementBatch, ProviderError> {
            Ok(MeasurementBatch {
                measurements: vec![],
                last_measurement_datetime: None,
                batch_size_limit_reached: false,
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
