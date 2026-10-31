use crate::core::domain::channels::channel::{Channel, value_objects};
use crate::core::domain::channels::repository_port::ChannelRepository;
use crate::core::domain::error::DomainError;

use super::pool::PgPool;

pub struct PostgresChannelRepository {
    pool: PgPool,
}

impl PostgresChannelRepository {
    pub fn new(pool: &PgPool) -> Self {
        Self { pool: pool.clone() }
    }

    fn map_row(row: &postgres::Row) -> Channel {
        Channel {
            id: value_objects::Id(row.get(0)),
            counting_station_id: value_objects::CountingStationId(row.get(1)),
            name: value_objects::Name(row.get(2)),
            description: value_objects::Description(row.get(3)),
            external_datasource_id: row
                .get::<_, Option<String>>(4)
                .map(value_objects::ExternalDatasourceId),
        }
    }
}

impl ChannelRepository for PostgresChannelRepository {
    fn save(&self, channel: Channel) -> Result<(), DomainError> {
        let mut client = self
            .pool
            .get()
            .map_err(|error| DomainError::Database(super::db_error(&error)))?;
        client
            .execute(
                "INSERT INTO channels (id, counting_station_id, name, description, external_datasource_id)
                 VALUES ($1, $2, $3, $4, $5)",
                &[
                    &channel.id.0,
                    &channel.counting_station_id.0,
                    &channel.name.0,
                    &channel.description.0,
                    &channel.external_datasource_id.as_ref().map(|id| id.0.as_str()),
                ],
            )
            .map_err(|error| DomainError::Database(super::db_error(&error)))?;
        Ok(())
    }

    fn update(&self, channel: Channel) -> Result<(), DomainError> {
        let mut client = self
            .pool
            .get()
            .map_err(|error| DomainError::Database(super::db_error(&error)))?;
        client
            .execute(
                "UPDATE channels
                 SET counting_station_id = $2, name = $3, description = $4
                 WHERE id = $1",
                &[
                    &channel.id.0,
                    &channel.counting_station_id.0,
                    &channel.name.0,
                    &channel.description.0,
                ],
            )
            .map_err(|error| DomainError::Database(super::db_error(&error)))?;
        Ok(())
    }

    fn find_by_id(&self, id: value_objects::Id) -> Result<Channel, DomainError> {
        let mut client = self
            .pool
            .get()
            .map_err(|error| DomainError::Database(super::db_error(&error)))?;
        let row = client
            .query_opt(
                "SELECT id, counting_station_id, name, description, external_datasource_id
                 FROM channels WHERE id = $1",
                &[&id.0],
            )
            .map_err(|error| DomainError::Database(super::db_error(&error)))?
            .ok_or(DomainError::NotFound(id.0))?;
        Ok(Self::map_row(&row))
    }

    fn find_all(&self) -> Result<Vec<Channel>, DomainError> {
        let mut client = self
            .pool
            .get()
            .map_err(|error| DomainError::Database(super::db_error(&error)))?;
        let rows = client
            .query(
                "SELECT id, counting_station_id, name, description, external_datasource_id
                 FROM channels ORDER BY name ASC",
                &[],
            )
            .map_err(|error| DomainError::Database(super::db_error(&error)))?;
        Ok(rows.iter().map(Self::map_row).collect())
    }

    fn find_by_counting_station_id(
        &self,
        station_id: value_objects::CountingStationId,
    ) -> Result<Vec<Channel>, DomainError> {
        let mut client = self
            .pool
            .get()
            .map_err(|error| DomainError::Database(super::db_error(&error)))?;
        let rows = client
            .query(
                "SELECT id, counting_station_id, name, description, external_datasource_id
                 FROM channels WHERE counting_station_id = $1 ORDER BY name ASC",
                &[&station_id.0],
            )
            .map_err(|error| DomainError::Database(super::db_error(&error)))?;
        Ok(rows.iter().map(Self::map_row).collect())
    }

    /// One set-based query for every channel of the requested stations: the
    /// viewport/summary read paths use this instead of loading the whole table.
    fn find_by_station_ids(
        &self,
        station_ids: &[value_objects::CountingStationId],
    ) -> Result<Vec<Channel>, DomainError> {
        if station_ids.is_empty() {
            return Ok(Vec::new());
        }
        let station_ids: Vec<uuid::Uuid> =
            station_ids.iter().map(|station_id| station_id.0).collect();
        let mut client = self
            .pool
            .get()
            .map_err(|error| DomainError::Database(super::db_error(&error)))?;
        let rows = client
            .query(
                "SELECT id, counting_station_id, name, description, external_datasource_id
                 FROM channels WHERE counting_station_id = ANY($1::uuid[]) ORDER BY name ASC",
                &[&station_ids],
            )
            .map_err(|error| DomainError::Database(super::db_error(&error)))?;
        Ok(rows.iter().map(Self::map_row).collect())
    }

    fn find_by_external_datasource_id(
        &self,
        external_id: value_objects::ExternalDatasourceId,
    ) -> Result<Option<Channel>, DomainError> {
        let mut client = self
            .pool
            .get()
            .map_err(|error| DomainError::Database(super::db_error(&error)))?;
        let row = client
            .query_opt(
                "SELECT id, counting_station_id, name, description, external_datasource_id
                 FROM channels WHERE external_datasource_id = $1",
                &[&external_id.0],
            )
            .map_err(|error| DomainError::Database(super::db_error(&error)))?;
        Ok(row.as_ref().map(Self::map_row))
    }

    fn find_filtered(
        &self,
        counting_station_id: Option<value_objects::CountingStationId>,
        name: Option<&str>,
    ) -> Result<Vec<Channel>, DomainError> {
        let mut client = self
            .pool
            .get()
            .map_err(|error| DomainError::Database(super::db_error(&error)))?;
        let rows = match (counting_station_id, name) {
            (Some(station_id), Some(name)) => client
                .query(
                    "SELECT id, counting_station_id, name, description, external_datasource_id
                     FROM channels
                     WHERE counting_station_id = $1 AND name ILIKE $2
                     ORDER BY name ASC",
                    &[&station_id.0, &format!("%{}%", escape_like(name))],
                )
                .map_err(|error| DomainError::Database(super::db_error(&error)))?,
            (Some(station_id), None) => client
                .query(
                    "SELECT id, counting_station_id, name, description, external_datasource_id
                     FROM channels WHERE counting_station_id = $1 ORDER BY name ASC",
                    &[&station_id.0],
                )
                .map_err(|error| DomainError::Database(super::db_error(&error)))?,
            (None, Some(name)) => client
                .query(
                    "SELECT id, counting_station_id, name, description, external_datasource_id
                     FROM channels WHERE name ILIKE $1 ORDER BY name ASC",
                    &[&format!("%{}%", escape_like(name))],
                )
                .map_err(|error| DomainError::Database(super::db_error(&error)))?,
            (None, None) => client
                .query(
                    "SELECT id, counting_station_id, name, description, external_datasource_id
                     FROM channels ORDER BY name ASC",
                    &[],
                )
                .map_err(|error| DomainError::Database(super::db_error(&error)))?,
        };
        Ok(rows.iter().map(Self::map_row).collect())
    }

    fn channel_ids_by_data_source_id(
        &self,
        data_source_id: crate::core::domain::counting_stations::counting_station::value_objects::DataSourceId,
    ) -> Result<Vec<value_objects::Id>, DomainError> {
        let mut client = self
            .pool
            .get()
            .map_err(|error| DomainError::Database(super::db_error(&error)))?;
        let rows = client
            .query(
                "SELECT c.id FROM channels c \
                 JOIN counting_stations s ON c.counting_station_id = s.id \
                 WHERE s.data_source_id = $1 ORDER BY c.name ASC",
                &[&data_source_id.0],
            )
            .map_err(|error| DomainError::Database(super::db_error(&error)))?;
        Ok(rows
            .iter()
            .map(|row| value_objects::Id(row.get::<_, uuid::Uuid>(0)))
            .collect())
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
    use uuid::Uuid;

    use super::*;
    use crate::adapter::driven::postgres::{
        PostgresCountingStationRepository, PostgresDataSourceRepository, create_pool,
    };
    use crate::core::domain::configuration::configuration::value_objects::DatabaseConfiguration;
    use crate::core::domain::counting_stations::counting_station::CountingStation;
    use crate::core::domain::counting_stations::counting_station::value_objects as station_vo;
    use crate::core::domain::counting_stations::repository_port::CountingStationRepository;
    use crate::core::domain::data_source::data_source::DataSource;
    use crate::core::domain::data_source::repository_port::DataSourceRepository;

    /// A running Postgres plus the channel/station/data-source repositories.
    struct TestDb {
        channels: PostgresChannelRepository,
        stations: PostgresCountingStationRepository,
        data_sources: PostgresDataSourceRepository,
        // Keep the container alive for the lifetime of the test.
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
                channels: PostgresChannelRepository::new(&pool),
                stations: PostgresCountingStationRepository::new(&pool),
                data_sources: PostgresDataSourceRepository::new(&pool),
                _container: container,
            }
        }

        /// Creates a data source (the station FK target) and returns its id.
        fn create_data_source(&self, name: &str) -> Uuid {
            let data_source =
                DataSource::new(name.to_string(), "eco_counter_v1_http_provider".to_string());
            self.data_sources.upsert(data_source.clone()).unwrap();
            data_source.id.0
        }

        /// Creates a station in the given data source.
        fn create_station(&self, data_source_id: Uuid, name: &str) -> Uuid {
            let station = CountingStation {
                id: station_vo::Id(Uuid::new_v4()),
                name: station_vo::Name(name.to_string()),
                description: station_vo::Description("desc".to_string()),
                external_datasource_id: None,
                data_source_id: Some(station_vo::DataSourceId(data_source_id)),
                coordinates: None,
                timezone: station_vo::Timezone("Europe/Berlin".to_string()),
                image_asset_id: None,
                image_sha256: None,
                status: station_vo::Status::Active,
            };
            let station_id = station.id.0;
            self.stations.save(station).unwrap();
            station_id
        }

        fn create_channel(&self, station_id: Uuid, name: &str) {
            self.channels
                .save(Channel {
                    id: value_objects::Id(Uuid::new_v4()),
                    counting_station_id: value_objects::CountingStationId(station_id),
                    name: value_objects::Name(name.to_string()),
                    description: value_objects::Description("desc".to_string()),
                    external_datasource_id: None,
                })
                .unwrap();
        }
    }

    #[test]
    fn find_by_station_ids_returns_only_the_requested_stations_in_name_order() {
        let db = TestDb::new();
        let data_source_id = db.create_data_source("Eco-Counter");
        let first = db.create_station(data_source_id, "Station A");
        let second = db.create_station(data_source_id, "Station B");
        db.create_channel(first, "out");
        db.create_channel(first, "in");
        db.create_channel(second, "both");

        let channels = db
            .channels
            .find_by_station_ids(&[value_objects::CountingStationId(first)])
            .unwrap();

        let names: Vec<&str> = channels
            .iter()
            .map(|channel| channel.name.0.as_str())
            .collect();
        assert_eq!(names, vec!["in", "out"], "one set-based query, name order");
        assert!(
            channels
                .iter()
                .all(|channel| channel.counting_station_id.0 == first),
            "channels of the other station must not be loaded"
        );

        let both = db
            .channels
            .find_by_station_ids(&[
                value_objects::CountingStationId(first),
                value_objects::CountingStationId(second),
            ])
            .unwrap();
        assert_eq!(both.len(), 3);
    }

    #[test]
    fn find_by_station_ids_skips_the_query_for_an_empty_slice() {
        let db = TestDb::new();
        assert!(db.channels.find_by_station_ids(&[]).unwrap().is_empty());
    }
}
