use crate::core::domain::counting_stations::counting_station::{CountingStation, value_objects};
use crate::core::domain::counting_stations::repository_port::CountingStationRepository;
use crate::core::domain::error::DomainError;

use super::pool::PgPool;

/// Shared column list for every counting-station read.
const STATION_COLUMNS: &str =
    "id, name, description, external_datasource_id, data_source_id, latitude, longitude";

pub struct PostgresCountingStationRepository {
    pool: PgPool,
}

impl PostgresCountingStationRepository {
    pub fn new(pool: &PgPool) -> Self {
        Self { pool: pool.clone() }
    }

    fn map_row(row: &postgres::Row) -> CountingStation {
        CountingStation {
            id: value_objects::Id(row.get(0)),
            name: value_objects::Name(row.get(1)),
            description: value_objects::Description(row.get(2)),
            external_datasource_id: row
                .get::<_, Option<String>>(3)
                .map(value_objects::ExternalDatasourceId),
            data_source_id: row
                .get::<_, Option<uuid::Uuid>>(4)
                .map(value_objects::DataSourceId),
            coordinates: match (row.get::<_, Option<f64>>(5), row.get::<_, Option<f64>>(6)) {
                (Some(latitude), Some(longitude)) => Some(value_objects::GeoCoordinates {
                    latitude,
                    longitude,
                }),
                _ => None,
            },
        }
    }
}

impl CountingStationRepository for PostgresCountingStationRepository {
    fn save(&self, station: CountingStation) -> Result<(), DomainError> {
        let mut client = self
            .pool
            .get()
            .map_err(|error| DomainError::Database(error.to_string()))?;
        client
            .execute(
                "INSERT INTO counting_stations (id, name, description, external_datasource_id, data_source_id, latitude, longitude)
                 VALUES ($1, $2, $3, $4, $5, $6, $7)",
                &[
                    &station.id.0,
                    &station.name.0,
                    &station.description.0,
                    &station
                        .external_datasource_id
                        .as_ref()
                        .map(|id| id.0.as_str()),
                    &station.data_source_id.map(|id| id.0),
                    &station.coordinates.map(|c| c.latitude),
                    &station.coordinates.map(|c| c.longitude),
                ],
            )
            .map_err(|error| DomainError::Database(error.to_string()))?;
        Ok(())
    }

    fn update(&self, station: CountingStation) -> Result<(), DomainError> {
        let mut client = self
            .pool
            .get()
            .map_err(|error| DomainError::Database(error.to_string()))?;
        client
            .execute(
                "UPDATE counting_stations
                 SET name = $2, description = $3, external_datasource_id = $4,
                     data_source_id = $5, latitude = $6, longitude = $7
                 WHERE id = $1",
                &[
                    &station.id.0,
                    &station.name.0,
                    &station.description.0,
                    &station
                        .external_datasource_id
                        .as_ref()
                        .map(|id| id.0.as_str()),
                    &station.data_source_id.map(|id| id.0),
                    &station.coordinates.map(|c| c.latitude),
                    &station.coordinates.map(|c| c.longitude),
                ],
            )
            .map_err(|error| DomainError::Database(error.to_string()))?;
        Ok(())
    }

    fn find_by_id(&self, id: value_objects::Id) -> Result<CountingStation, DomainError> {
        let mut client = self
            .pool
            .get()
            .map_err(|error| DomainError::Database(error.to_string()))?;
        let row = client
            .query_opt(
                &format!("SELECT {STATION_COLUMNS} FROM counting_stations WHERE id = $1"),
                &[&id.0],
            )
            .map_err(|error| DomainError::Database(error.to_string()))?
            .ok_or(DomainError::NotFound(id.0))?;
        Ok(Self::map_row(&row))
    }

    fn find_all(&self) -> Result<Vec<CountingStation>, DomainError> {
        let mut client = self
            .pool
            .get()
            .map_err(|error| DomainError::Database(error.to_string()))?;
        let rows = client
            .query(
                &format!("SELECT {STATION_COLUMNS} FROM counting_stations ORDER BY name ASC"),
                &[],
            )
            .map_err(|error| DomainError::Database(error.to_string()))?;
        Ok(rows.iter().map(Self::map_row).collect())
    }

    fn find_by_external_datasource_id(
        &self,
        external_id: value_objects::ExternalDatasourceId,
    ) -> Result<Option<CountingStation>, DomainError> {
        let mut client = self
            .pool
            .get()
            .map_err(|error| DomainError::Database(error.to_string()))?;
        let row = client
            .query_opt(
                &format!(
                    "SELECT {STATION_COLUMNS} FROM counting_stations WHERE external_datasource_id = $1"
                ),
                &[&external_id.0],
            )
            .map_err(|error| DomainError::Database(error.to_string()))?;
        Ok(row.as_ref().map(Self::map_row))
    }

    fn find_filtered(&self, name: Option<&str>) -> Result<Vec<CountingStation>, DomainError> {
        let mut client = self
            .pool
            .get()
            .map_err(|error| DomainError::Database(error.to_string()))?;
        let rows = match name {
            Some(name) => client
                .query(
                    &format!(
                        "SELECT {STATION_COLUMNS} FROM counting_stations WHERE name ILIKE $1 ORDER BY name ASC"
                    ),
                    &[&format!("%{}%", escape_like(name))],
                )
                .map_err(|error| DomainError::Database(error.to_string()))?,
            None => client
                .query(
                    &format!("SELECT {STATION_COLUMNS} FROM counting_stations ORDER BY name ASC"),
                    &[],
                )
                .map_err(|error| DomainError::Database(error.to_string()))?,
        };
        Ok(rows.iter().map(Self::map_row).collect())
    }
}

/// Escapes `LIKE`/`ILIKE` wildcards in a user-supplied substring so it is
/// matched literally.
fn escape_like(value: &str) -> String {
    value
        .replace('\\', "\\\\")
        .replace('%', "\\%")
        .replace('_', "\\_")
}

#[cfg(test)]
mod tests {
    use testcontainers::runners::SyncRunner;
    use testcontainers_modules::postgres::Postgres;

    use super::PostgresCountingStationRepository;
    use crate::adapter::driven::postgres::PostgresDataSourceRepository;
    use crate::adapter::driven::postgres::create_pool;
    use crate::core::domain::configuration::configuration::value_objects::DatabaseConfiguration;
    use crate::core::domain::counting_stations::counting_station::CountingStation;
    use crate::core::domain::counting_stations::counting_station::value_objects as station_vo;
    use crate::core::domain::counting_stations::repository_port::CountingStationRepository;
    use crate::core::domain::data_source::data_source::DataSource;
    use crate::core::domain::data_source::data_source::value_objects::Id;
    use crate::core::domain::data_source::repository_port::DataSourceRepository;
    use uuid::Uuid;

    /// A running Postgres test instance plus the counting-station repository,
    /// plus a data-source repository so the `data_source_id` NOT NULL FK can be
    /// satisfied.
    struct TestDb {
        repository: PostgresCountingStationRepository,
        data_source_repository: PostgresDataSourceRepository,
        // Keep the container handle alive for the lifetime of the test:
        // dropping it would stop the container and close the connection.
        _container: testcontainers::Container<Postgres>,
    }

    impl TestDb {
        fn new() -> Self {
            let database_user = "bike_counter_test_user";
            let database_password = "bike_counter_test_password";
            let database_name = "bike_counter_test";
            let container = Postgres::default()
                .with_user(database_user)
                .with_password(database_password)
                .with_db_name(database_name)
                .start()
                .unwrap();
            let url = format!(
                "postgres://127.0.0.1:{}/{}",
                container.get_host_port_ipv4(5432).unwrap(),
                database_name
            );
            let configuration = DatabaseConfiguration::new(
                url,
                database_user.to_string(),
                database_password.to_string(),
                database_name.to_string(),
            )
            .unwrap();
            let pool = create_pool(&configuration).unwrap();
            Self {
                repository: PostgresCountingStationRepository::new(&pool),
                data_source_repository: PostgresDataSourceRepository::new(&pool),
                _container: container,
            }
        }

        /// Inserts a data source row (the FK target) and returns its id.
        fn create_data_source(&self, name: &str) -> Id {
            let data_source = DataSource::new(
                name.to_string(),
                "münster_opendata_github_provider".to_string(),
            );
            self.data_source_repository
                .upsert(data_source.clone())
                .unwrap();
            data_source.id
        }
    }

    #[test]
    fn save_find_and_update_coordinates_round_trip() {
        let db = TestDb::new();
        let data_source_id = db.create_data_source("Münster");
        let mut station = CountingStation {
            id: station_vo::Id(Uuid::new_v4()),
            name: station_vo::Name("Promenade".to_string()),
            description: station_vo::Description("desc".to_string()),
            external_datasource_id: Some(station_vo::ExternalDatasourceId("100031297".to_string())),
            data_source_id: Some(station_vo::DataSourceId(data_source_id.0)),
            coordinates: None,
        };
        db.repository.save(station.clone()).unwrap();

        let stored = db.repository.find_by_id(station.id).unwrap();
        assert_eq!(stored.name.0, "Promenade");
        assert_eq!(stored.coordinates, None);

        // Attach coordinates through the repository update path.
        station.coordinates = Some(station_vo::GeoCoordinates {
            latitude: 51.9617,
            longitude: 7.6335,
        });
        db.repository.update(station.clone()).unwrap();

        let updated = db.repository.find_by_id(station.id).unwrap();
        assert_eq!(
            updated.coordinates,
            Some(station_vo::GeoCoordinates {
                latitude: 51.9617,
                longitude: 7.6335,
            })
        );

        // Clear them back to "not provided".
        station.coordinates = None;
        db.repository.update(station.clone()).unwrap();
        let cleared = db.repository.find_by_id(station.id).unwrap();
        assert_eq!(cleared.coordinates, None);
    }

    #[test]
    fn find_all_returns_persisted_coordinates() {
        let db = TestDb::new();
        let data_source_id = db.create_data_source("Münster");
        let station = CountingStation {
            id: station_vo::Id(Uuid::new_v4()),
            name: station_vo::Name("Neutor".to_string()),
            description: station_vo::Description("desc".to_string()),
            external_datasource_id: Some(station_vo::ExternalDatasourceId("100035541".to_string())),
            data_source_id: Some(station_vo::DataSourceId(data_source_id.0)),
            coordinates: Some(station_vo::GeoCoordinates {
                latitude: 51.9673,
                longitude: 7.6184,
            }),
        };
        db.repository.save(station.clone()).unwrap();

        let all = db.repository.find_all().unwrap();
        assert_eq!(all.len(), 1);
        assert_eq!(all[0].coordinates, station.coordinates);
    }
}
