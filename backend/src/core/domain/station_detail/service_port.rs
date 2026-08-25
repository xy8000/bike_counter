//! Driving (inbound) port for the station-detail aggregation. Implemented by
//! `StationDetailService`; consumed by the BFF handlers.

use chrono::{DateTime, Utc};

use crate::core::domain::counting_stations::counting_station::value_objects::Id;
use crate::core::domain::error::DomainError;
use crate::core::domain::station_detail::StationDetail;

pub trait StationDetailServicePort: Send + Sync {
    /// Everything the detail page needs beyond the overview: the channels and
    /// the bucketed time-series graphs, computed over the station's timezone.
    fn detail(&self, station_id: Id, now: DateTime<Utc>) -> Result<StationDetail, DomainError>;
}
