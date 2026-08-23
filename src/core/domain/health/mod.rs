//! Business domain module for service health.
//!
//! - Models: [`indicator::HealthStatus`], [`indicator::HealthComponent`].
//! - Driven port: [`indicator_port::ServiceHealthIndicator`] (implemented by
//!   `PostgresHealthCheck` and [`provider_health_indicator::ProviderHealthIndicator`]).
//! - Driving port: [`service_port::HealthServicePort`] (implemented by
//!   [`service::HealthService`]).
//! - Domain service: [`service::HealthService`].

pub mod indicator;
pub mod indicator_port;
pub mod provider_health_indicator;
pub mod service;
pub mod service_port;

pub use indicator::{HealthComponent, HealthStatus};
pub use indicator_port::ServiceHealthIndicator;
pub use service::HealthService;
