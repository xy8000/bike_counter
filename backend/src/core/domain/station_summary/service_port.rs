//! Driving (inbound) port for the station-summary aggregation. Implemented by
//! `StationSummaryService`; consumed by the BFF handlers.

use chrono::{DateTime, Utc};

use crate::core::domain::error::DomainError;
use crate::core::domain::station_summary::StationSummary;
use crate::core::domain::station_summary::bounds::GeoBounds;

pub trait StationSummaryServicePort: Send + Sync {
    /// Summaries for every station, optionally restricted to those whose
    /// coordinates lie inside `bounds`, for measurements between `from`
    /// (inclusive) and `to` (inclusive).
    ///
    /// `None` bounds return every station (used by the search dialog); `Some`
    /// returns only the stations inside the bounding box (used by the sidebar).
    fn summarize(
        &self,
        bounds: Option<GeoBounds>,
        from: DateTime<Utc>,
        to: DateTime<Utc>,
    ) -> Result<Vec<StationSummary>, DomainError>;
}
