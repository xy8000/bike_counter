//! Driven (outbound) port: a named health check for a downstream service.
//!
//! Implemented by the driven adapter (`PostgresHealthCheck` for the database
//! probe) and by [`ProviderHealthIndicator`](super::provider_health_indicator)
//! for each configured data source; consumed by
//! [`HealthService`](super::service) to build the readiness report.
//!
//! The [`HealthStatus`] / [`HealthComponent`] models live in the sibling
//! `indicator.rs`.

/// Checks the health of a downstream service.
///
/// Implementations are synchronous so they can be run inside
/// `tokio::task::spawn_blocking` (the `postgres` crate must not run on a
/// tokio worker thread).
pub trait ServiceHealthIndicator: Send + Sync {
    fn name(&self) -> String;

    fn check(&self) -> crate::core::domain::health::indicator::HealthStatus;
}
