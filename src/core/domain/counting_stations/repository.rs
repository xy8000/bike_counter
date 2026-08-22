use super::counting_station::{CountingStation, value_objects};
use crate::core::domain::error::DomainError;

pub trait CountingStationRepository {
    fn save(&self, station: CountingStation) -> Result<(), DomainError>;
    fn find_by_id(&self, id: value_objects::Id) -> Result<CountingStation, DomainError>;
}
