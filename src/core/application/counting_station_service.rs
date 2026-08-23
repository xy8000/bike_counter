//! Application service exposing counting-station reads through the core.

use std::sync::Arc;

use crate::core::domain::counting_stations::counting_station::CountingStation;
use crate::core::domain::counting_stations::counting_station::value_objects as station_vo;
use crate::core::domain::counting_stations::repository::CountingStationRepository;
use crate::core::domain::error::DomainError;

pub struct CountingStationService {
    repository: Arc<dyn CountingStationRepository + Send + Sync>,
}

impl CountingStationService {
    pub fn new(repository: Arc<dyn CountingStationRepository + Send + Sync>) -> Self {
        Self { repository }
    }

    /// Lists all counting stations.
    pub fn list(&self) -> Result<Vec<CountingStation>, DomainError> {
        self.repository.find_all()
    }

    /// Returns a single counting station; `DomainError::NotFound` if unknown.
    pub fn find_by_id(&self, id: station_vo::Id) -> Result<CountingStation, DomainError> {
        self.repository.find_by_id(id)
    }
}

#[cfg(test)]
mod tests {
    use std::sync::Arc;

    use uuid::Uuid;

    use super::CountingStationService;
    use crate::core::domain::counting_stations::counting_station::CountingStation;
    use crate::core::domain::counting_stations::counting_station::value_objects as station_vo;
    use crate::core::domain::counting_stations::repository::CountingStationRepository;
    use crate::core::domain::error::DomainError;

    struct MemoryCountingStationRepository {
        stations: Vec<CountingStation>,
    }

    impl CountingStationRepository for MemoryCountingStationRepository {
        fn save(&self, _station: CountingStation) -> Result<(), DomainError> {
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
    }

    fn station(id: Uuid, name: &str) -> CountingStation {
        CountingStation {
            id: station_vo::Id(id),
            name: station_vo::Name(name.to_string()),
            description: station_vo::Description(String::new()),
            external_datasource_id: None,
            data_source_id: None,
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
        let stations = service().list().unwrap();
        assert_eq!(stations.len(), 2);
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
}
