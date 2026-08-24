//! Aggregates individual service health indicators into a readiness report.

use std::sync::Arc;

use super::indicator::HealthComponent;
use super::indicator_port::ServiceHealthIndicator;
use crate::core::domain::health::service_port::HealthServicePort;

/// Runs all registered downstream health checks and aggregates the results.
pub struct HealthService {
    indicators: Vec<Arc<dyn ServiceHealthIndicator>>,
}

impl HealthService {
    pub fn new(indicators: Vec<Arc<dyn ServiceHealthIndicator>>) -> Self {
        Self { indicators }
    }

    /// Runs every indicator and returns the per-component results.
    pub fn check(&self) -> Vec<HealthComponent> {
        self.indicators
            .iter()
            .map(|indicator| HealthComponent {
                name: indicator.name(),
                status: indicator.check(),
            })
            .collect()
    }
}

impl HealthServicePort for HealthService {
    fn check(&self) -> Vec<HealthComponent> {
        self.check()
    }
}

#[cfg(test)]
mod tests {
    use std::sync::Arc;

    use super::HealthService;
    use crate::core::domain::health::{HealthStatus, ServiceHealthIndicator};

    struct MockIndicator {
        name: String,
        status: HealthStatus,
    }

    impl ServiceHealthIndicator for MockIndicator {
        fn name(&self) -> String {
            self.name.clone()
        }

        fn check(&self) -> HealthStatus {
            self.status.clone()
        }
    }

    #[test]
    fn reports_every_component_in_order() {
        let service = HealthService::new(vec![
            Arc::new(MockIndicator {
                name: "postgres".to_string(),
                status: HealthStatus::Up,
            }),
            Arc::new(MockIndicator {
                name: "redis".to_string(),
                status: HealthStatus::Down("no route to host".to_string()),
            }),
        ]);

        let components = service.check();
        assert_eq!(components.len(), 2);
        assert_eq!(components[0].name, "postgres");
        assert!(components[0].status.is_up());
        assert_eq!(components[1].name, "redis");
        assert_eq!(
            components[1].status,
            HealthStatus::Down("no route to host".to_string())
        );
    }

    #[test]
    fn returns_empty_report_without_indicators() {
        let service = HealthService::new(vec![]);
        assert!(service.check().is_empty());
    }
}
