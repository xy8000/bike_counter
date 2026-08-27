//! Application service for all station analytics aggregations behind the BFF
//! endpoints.
//!
//! [`service::StationAnalyticsService`] is the single orchestrator: it fetches
//! stations/channels and assembles the five BFF payloads. The heavy aggregation
//! lives in two helper modules:
//!
//! - [`metrics`] — the four overview metrics (day / 7 days / month / year).
//! - [`graphs`] — the bucketed detail/summary time-series graphs and radars.

pub mod graphs;
pub mod metrics;
pub mod resolution;
pub mod service;

#[cfg(test)]
mod tests;

pub use service::StationAnalyticsService;
