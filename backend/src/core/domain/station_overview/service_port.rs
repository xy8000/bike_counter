//! Driving (inbound) port for the station-overview aggregation. Implemented by
//! `StationOverviewService`; consumed by the BFF handlers.

use chrono::{DateTime, Utc};

use crate::core::domain::counting_stations::counting_station::value_objects::Id;
use crate::core::domain::error::DomainError;
use crate::core::domain::station_overview::StationOverview;

pub trait StationOverviewServicePort: Send + Sync {
    /// Everything the overview panel needs for one station, computed over
    /// complete calendar periods in the station's own timezone from `now`.
    fn overview(&self, station_id: Id, now: DateTime<Utc>) -> Result<StationOverview, DomainError>;
}
