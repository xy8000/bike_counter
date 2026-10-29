//! Aggregates individual service health indicators into a readiness report.

use std::sync::Arc;

use super::indicator::HealthComponent;
use super::indicator_port::ServiceHealthIndicator;
use crate::core::domain::health::service_port::HealthServicePort;

/// Runs the registered downstream health checks and aggregates the results.
///
/// Two independent indicator sets are kept apart on purpose:
///
/// - **critical** — gate the readiness report ([`check`](Self::check)). Only
///   internal dependencies (the database) belong here, so readiness never
///   depends on external provider URLs.
/// - **informational** — reported by
///   [`check_details`](Self::check_details) only, e.g. one reachability probe
///   per configured external data source. They never gate readiness and are
///   never consulted for the readiness verdict.
pub struct HealthService {
    critical: Vec<Arc<dyn ServiceHealthIndicator>>,
    informational: Vec<Arc<dyn ServiceHealthIndicator>>,
}

impl HealthService {
    /// Builds a service whose indicators all gate readiness and which has no
    /// informational indicators.
    pub fn new(critical: Vec<Arc<dyn ServiceHealthIndicator>>) -> Self {
        Self {
            critical,
            informational: Vec::new(),
        }
    }

    /// Builds a service with a separate informational set that is reported by
    /// [`check_details`](Self::check_details) without affecting readiness.
    pub fn with_informational(
        critical: Vec<Arc<dyn ServiceHealthIndicator>>,
        informational: Vec<Arc<dyn ServiceHealthIndicator>>,
    ) -> Self {
        Self {
            critical,
            informational,
        }
    }

    /// Runs every critical indicator and returns the per-component results.
    pub fn check(&self) -> Vec<HealthComponent> {
        run(&self.critical)
    }

    /// Runs every informational indicator and returns the per-component
    /// results. This is diagnostics only: the result must never change the
    /// readiness verdict or the HTTP status of the readiness endpoint.
    pub fn check_details(&self) -> Vec<HealthComponent> {
        run(&self.informational)
    }
}

/// Runs the given indicators in registration order.
fn run(indicators: &[Arc<dyn ServiceHealthIndicator>]) -> Vec<HealthComponent> {
    indicators
        .iter()
        .map(|indicator| HealthComponent {
            name: indicator.name(),
            status: indicator.check(),
        })
        .collect()
}

impl HealthServicePort for HealthService {
    fn check(&self) -> Vec<HealthComponent> {
        self.check()
    }

    fn check_details(&self) -> Vec<HealthComponent> {
        self.check_details()
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

    fn indicator(name: &str, status: HealthStatus) -> Arc<MockIndicator> {
        Arc::new(MockIndicator {
            name: name.to_string(),
            status,
        })
    }

    #[test]
    fn reports_every_component_in_order() {
        let service = HealthService::new(vec![
            indicator("postgres", HealthStatus::Up),
            indicator("redis", HealthStatus::Down("no route to host".to_string())),
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

    #[test]
    fn check_ignores_informational_indicators() {
        let service = HealthService::with_informational(
            vec![indicator("postgres", HealthStatus::Up)],
            vec![indicator(
                "provider",
                HealthStatus::Down("offline".to_string()),
            )],
        );

        let critical = service.check();
        assert_eq!(critical.len(), 1, "only the critical set gates readiness");
        assert_eq!(critical[0].name, "postgres");
        assert!(critical[0].status.is_up());
    }

    #[test]
    fn check_details_reports_only_informational_indicators() {
        let service = HealthService::with_informational(
            vec![indicator("postgres", HealthStatus::Up)],
            vec![
                indicator("provider-a", HealthStatus::Up),
                indicator("provider-b", HealthStatus::Down("offline".to_string())),
            ],
        );

        let details = service.check_details();
        assert_eq!(details.len(), 2);
        assert_eq!(details[0].name, "provider-a");
        assert!(details[0].status.is_up());
        assert_eq!(details[1].name, "provider-b");
        assert_eq!(details[1].status, HealthStatus::Down("offline".to_string()));
    }

    #[test]
    fn details_are_empty_without_informational_indicators() {
        let service = HealthService::new(vec![indicator("postgres", HealthStatus::Up)]);
        assert!(service.check_details().is_empty());
    }
}
