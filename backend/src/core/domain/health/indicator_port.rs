//! Driven (outbound) port: a named health check for a downstream service.
//!
//! Implemented by the driven adapter (`PostgresHealthCheck` for the database
//! probe, a **critical** indicator that gates readiness) and by
//! [`ProviderHealthIndicator`](super::provider_health_indicator) for each
//! configured data source (an **informational** indicator reported by
//! `/health/providers`, which never gates readiness); consumed by
//! [`HealthService`](super::service).
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
