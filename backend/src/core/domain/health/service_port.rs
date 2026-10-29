//! Driving (inbound) port for the health report. Implemented by
//! [`HealthService`](super::service); consumed by the REST health handlers.

use crate::core::domain::health::indicator::HealthComponent;

pub trait HealthServicePort: Send + Sync {
    /// Runs every **critical** indicator and returns the per-component results.
    /// This is the readiness verdict: it must only cover internal dependencies
    /// and never external provider URLs.
    fn check(&self) -> Vec<HealthComponent>;

    /// Runs every **informational** indicator (e.g. external data-source
    /// reachability) and returns the per-component results. Diagnostics only —
    /// the result must never gate readiness or change its HTTP status.
    fn check_details(&self) -> Vec<HealthComponent>;
}
