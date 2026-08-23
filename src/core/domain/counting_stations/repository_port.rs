use super::counting_station::{CountingStation, value_objects};
use crate::core::domain::error::DomainError;

pub trait CountingStationRepository {
    fn save(&self, station: CountingStation) -> Result<(), DomainError>;
    fn find_by_id(&self, id: value_objects::Id) -> Result<CountingStation, DomainError>;
    fn find_all(&self) -> Result<Vec<CountingStation>, DomainError>;
    fn find_by_external_datasource_id(
        &self,
        external_id: value_objects::ExternalDatasourceId,
    ) -> Result<Option<CountingStation>, DomainError>;

    /// Lists counting stations, optionally filtered by a case-insensitive
    /// name substring.
    fn find_filtered(&self, name: Option<&str>) -> Result<Vec<CountingStation>, DomainError>;
}
