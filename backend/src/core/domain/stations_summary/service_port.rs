//! Driving (inbound) port for the station-summary aggregation. Implemented by
//! `StationsSummaryService`; consumed by the BFF `stations/summary` handler.

use chrono::{DateTime, Utc};

use crate::core::domain::counting_stations::counting_station::value_objects::Id;
use crate::core::domain::error::DomainError;
use crate::core::domain::station_summary::bounds::GeoBounds;
use crate::core::domain::stations_summary::StationsSummary;

pub trait StationsSummaryServicePort: Send + Sync {
    /// Computes the aggregated summary page for every positioned station inside
    /// `bounds`, excluding the given station ids from the aggregation (the
    /// excluded stations still appear in the returned `stations` list so the
    /// frontend can gray them out on the map).
    fn summarize(
        &self,
        bounds: GeoBounds,
        exclude: &[Id],
        now: DateTime<Utc>,
    ) -> Result<StationsSummary, DomainError>;
}
