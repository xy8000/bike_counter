//! Business domain module for the station-summary aggregation behind the BFF's
//! "visible stations" endpoints.
//!
//! - Model: [`StationSummary`] and [`bounds::GeoBounds`].
//! - Driving port: [`service_port::StationSummaryServicePort`] (implemented by
//!   `StationSummaryService`).

use crate::core::domain::counting_stations::counting_station::CountingStation;

/// A counting station enriched with its channel count and the number of
/// bikes measured across all of its channels in the last 24 hours.
#[derive(Debug, Clone)]
pub struct StationSummary {
    pub station: CountingStation,
    pub channel_count: usize,
    pub bikes_last_24h: i64,
}

pub mod bounds;
pub mod service_port;
