//! Driving (inbound) port for the health report. Implemented by
//! [`HealthService`](super::service); consumed by the REST health handlers.

use crate::core::domain::health::indicator::HealthComponent;

pub trait HealthServicePort: Send + Sync {
    /// Runs every indicator and returns the per-component results.
    fn check(&self) -> Vec<HealthComponent>;
}
