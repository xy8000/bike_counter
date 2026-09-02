//! Application service exposing counting-station reads through the core.

use std::sync::Arc;

use crate::core::domain::counting_stations::counting_station::CountingStation;
use crate::core::domain::counting_stations::counting_station::value_objects as station_vo;
use crate::core::domain::counting_stations::repository_port::CountingStationRepository;
use crate::core::domain::counting_stations::service_port::CountingStationServicePort;
use crate::core::domain::error::DomainError;

pub struct CountingStationService {
    repository: Arc<dyn CountingStationRepository + Send + Sync>,
}

impl CountingStationService {
    pub fn new(repository: Arc<dyn CountingStationRepository + Send + Sync>) -> Self {
        Self { repository }
    }

    /// Lists counting stations, optionally filtered by a case-insensitive
    /// name substring.
    pub fn list(&self, name: Option<&str>) -> Result<Vec<CountingStation>, DomainError> {
        self.repository.find_filtered(name)
    }

    /// Counts all counting stations (no rows transferred).
    pub fn count_all(&self) -> Result<usize, DomainError> {
        self.repository.count_all()
    }

    /// Lists the **positioned** counting stations whose coordinates lie inside
    /// the given axis-aligned bounding box (the map viewport). The filter is
    /// pushed into the repository, so a viewport read never loads the whole
    /// table.
    pub fn list_in_bounds(
        &self,
        min_latitude: f64,
        min_longitude: f64,
        max_latitude: f64,
        max_longitude: f64,
    ) -> Result<Vec<CountingStation>, DomainError> {
        self.repository
            .find_in_bounds(min_latitude, min_longitude, max_latitude, max_longitude)
    }

    /// Returns a single counting station; `DomainError::NotFound` if unknown.
    pub fn find_by_id(&self, id: station_vo::Id) -> Result<CountingStation, DomainError> {
        self.repository.find_by_id(id)
    }

    /// Sets the GPS coordinates of a counting station; `None` clears them.
    /// Returns the updated station; `DomainError::NotFound` if unknown.
    pub fn update_coordinates(
        &self,
        id: station_vo::Id,
        coordinates: Option<station_vo::GeoCoordinates>,
    ) -> Result<CountingStation, DomainError> {
        let mut station = self.repository.find_by_id(id)?;
        station.coordinates = coordinates;
        self.repository.update(station.clone())?;
        Ok(station)
    }
}

impl CountingStationServicePort for CountingStationService {
    fn list(&self, name: Option<&str>) -> Result<Vec<CountingStation>, DomainError> {
        self.list(name)
    }

    fn count_all(&self) -> Result<usize, DomainError> {
        self.count_all()
    }

    fn list_in_bounds(
        &self,
        min_latitude: f64,
        min_longitude: f64,
        max_latitude: f64,
        max_longitude: f64,
    ) -> Result<Vec<CountingStation>, DomainError> {
        self.list_in_bounds(min_latitude, min_longitude, max_latitude, max_longitude)
    }

    fn find_by_id(&self, id: station_vo::Id) -> Result<CountingStation, DomainError> {
        self.find_by_id(id)
    }

    fn update_coordinates(
        &self,
        id: station_vo::Id,
        coordinates: Option<station_vo::GeoCoordinates>,
    ) -> Result<CountingStation, DomainError> {
        self.update_coordinates(id, coordinates)
    }
}

#[cfg(test)]
mod tests {
    use std::sync::Arc;

    use uuid::Uuid;

    use super::CountingStationService;
    use crate::core::domain::counting_stations::counting_station::CountingStation;
    use crate::core::domain::counting_stations::counting_station::value_objects as station_vo;
    use crate::core::domain::counting_stations::repository_port::CountingStationRepository;
    use crate::core::domain::error::DomainError;

    struct MemoryCountingStationRepository {
        stations: Vec<CountingStation>,
    }

    impl CountingStationRepository for MemoryCountingStationRepository {
        fn save(&self, _station: CountingStation) -> Result<(), DomainError> {
            Ok(())
        }

        fn update(&self, _station: CountingStation) -> Result<(), DomainError> {
            Ok(())
        }

        fn find_by_id(&self, id: station_vo::Id) -> Result<CountingStation, DomainError> {
            self.stations
                .iter()
                .find(|station| station.id.0 == id.0)
                .cloned()
                .ok_or(DomainError::NotFound(id.0))
        }

        fn find_all(&self) -> Result<Vec<CountingStation>, DomainError> {
            Ok(self.stations.clone())
        }

        fn find_by_external_datasource_id(
            &self,
            _external_id: station_vo::ExternalDatasourceId,
        ) -> Result<Option<CountingStation>, DomainError> {
            Ok(None)
        }

        fn find_filtered(&self, name: Option<&str>) -> Result<Vec<CountingStation>, DomainError> {
            Ok(match name {
                Some(name) => self
                    .stations
                    .iter()
                    .filter(|s| s.name.0.to_lowercase().contains(&name.to_lowercase()))
                    .cloned()
                    .collect(),
                None => self.stations.clone(),
            })
        }
    }

    fn station(id: Uuid, name: &str) -> CountingStation {
        CountingStation {
            id: station_vo::Id(id),
            name: station_vo::Name(name.to_string()),
            description: station_vo::Description(String::new()),
            external_datasource_id: None,
            data_source_id: None,
            coordinates: None,
            timezone: station_vo::Timezone("Europe/Berlin".to_string()),
            image_asset_id: None,
            image_sha256: None,
            status: station_vo::Status::Active,
        }
    }

    fn service() -> CountingStationService {
        CountingStationService::new(Arc::new(MemoryCountingStationRepository {
            stations: vec![
                station(Uuid::from_u128(0x1), "A"),
                station(Uuid::from_u128(0x2), "B"),
            ],
        }))
    }

    #[test]
    fn list_returns_all_stations() {
        let stations = service().list(None).unwrap();
        assert_eq!(stations.len(), 2);
    }

    #[test]
    fn list_filters_by_case_insensitive_name_substring() {
        let stations = service().list(Some("a")).unwrap();
        assert_eq!(stations.len(), 1);
        assert_eq!(stations[0].name.0, "A");
    }

    #[test]
    fn find_by_id_returns_the_station() {
        let station = service()
            .find_by_id(station_vo::Id(Uuid::from_u128(0x1)))
            .unwrap();
        assert_eq!(station.name.0, "A");
    }

    #[test]
    fn find_by_unknown_id_is_not_found() {
        assert!(matches!(
            service().find_by_id(station_vo::Id(Uuid::from_u128(0x99))),
            Err(DomainError::NotFound(_))
        ));
    }

    #[test]
    fn update_coordinates_sets_the_coordinates_on_the_station() {
        let updated = service()
            .update_coordinates(
                station_vo::Id(Uuid::from_u128(0x1)),
                Some(station_vo::GeoCoordinates {
                    latitude: 51.9565,
                    longitude: 7.6152,
                }),
            )
            .unwrap();
        assert_eq!(updated.name.0, "A");
        assert_eq!(
            updated.coordinates,
            Some(station_vo::GeoCoordinates {
                latitude: 51.9565,
                longitude: 7.6152,
            })
        );
    }

    #[test]
    fn update_coordinates_can_clear_them_to_not_provided() {
        let updated = service()
            .update_coordinates(station_vo::Id(Uuid::from_u128(0x1)), None)
            .unwrap();
        assert_eq!(updated.coordinates, None);
    }

    #[test]
    fn update_coordinates_unknown_station_is_not_found() {
        assert!(matches!(
            service().update_coordinates(station_vo::Id(Uuid::from_u128(0x99)), None),
            Err(DomainError::NotFound(_))
        ));
    }
}
