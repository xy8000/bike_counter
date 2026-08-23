//! Driving (inbound) port for counting-station reads. Implemented by
//! `CountingStationService`; consumed by the REST counting-stations handlers.

use crate::core::domain::counting_stations::counting_station::CountingStation;
use crate::core::domain::counting_stations::counting_station::value_objects as station_vo;
use crate::core::domain::error::DomainError;

pub trait CountingStationServicePort: Send + Sync {
    /// Lists counting stations, optionally filtered by a case-insensitive name
    /// substring.
    fn list(&self, name: Option<&str>) -> Result<Vec<CountingStation>, DomainError>;

    /// Returns a single counting station; `DomainError::NotFound` if unknown.
    fn find_by_id(&self, id: station_vo::Id) -> Result<CountingStation, DomainError>;
}
