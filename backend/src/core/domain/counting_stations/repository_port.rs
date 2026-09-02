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

    /// Lists the **positioned** counting stations whose coordinates lie inside
    /// the given axis-aligned bounding box (the map viewport). The Postgres
    /// adapter pushes the filter into the `WHERE` clause so a viewport read never
    /// loads the whole table; the default implementation filters
    /// [`find_all`](Self::find_all) in memory and is intended for in-memory test
    /// doubles.
    fn find_in_bounds(
        &self,
        min_latitude: f64,
        min_longitude: f64,
        max_latitude: f64,
        max_longitude: f64,
    ) -> Result<Vec<CountingStation>, DomainError> {
        Ok(self
            .find_all()?
            .into_iter()
            .filter(|station| {
                station.coordinates.is_some_and(|c| {
                    c.latitude >= min_latitude
                        && c.latitude <= max_latitude
                        && c.longitude >= min_longitude
                        && c.longitude <= max_longitude
                })
            })
            .collect())
    }

    /// Counts all counting stations. The Postgres adapter uses `SELECT count(*)`
    /// so counting never transfers the rows; the default implementation counts
    /// [`find_all`](Self::find_all) and is intended for in-memory test doubles.
    fn count_all(&self) -> Result<usize, DomainError> {
        Ok(self.find_all()?.len())
    }

    /// Lists the counting stations of one data source. The Postgres adapter
    /// pushes the filter into the `WHERE` clause; the default implementation
    /// filters [`find_all`](Self::find_all) in memory and is intended for
    /// in-memory test doubles.
    fn find_by_data_source_id(
        &self,
        data_source_id: value_objects::DataSourceId,
    ) -> Result<Vec<CountingStation>, DomainError> {
        Ok(self
            .find_all()?
            .into_iter()
            .filter(|station| station.data_source_id == Some(data_source_id))
            .collect())
    }

    /// Counts the counting stations of one data source. The Postgres adapter
    /// uses `SELECT count(*)`; the default implementation counts
    /// [`find_by_data_source_id`](Self::find_by_data_source_id) and is intended
    /// for in-memory test doubles.
    fn count_by_data_source_id(
        &self,
        data_source_id: value_objects::DataSourceId,
    ) -> Result<usize, DomainError> {
        Ok(self.find_by_data_source_id(data_source_id)?.len())
    }

    /// Updates the mutable attributes (name, description, coordinates) of an
    /// existing station, keyed by its id.
    fn update(&self, station: CountingStation) -> Result<(), DomainError>;
}
