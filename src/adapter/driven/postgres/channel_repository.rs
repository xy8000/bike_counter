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
            .map_err(|error| DomainError::Database(error.to_string()))?;
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
            .map_err(|error| DomainError::Database(error.to_string()))?;
        Ok(())
    }

    fn find_by_id(&self, id: value_objects::Id) -> Result<Channel, DomainError> {
        let mut client = self
            .pool
            .get()
            .map_err(|error| DomainError::Database(error.to_string()))?;
        let row = client
            .query_opt(
                "SELECT id, counting_station_id, name, description, external_datasource_id
                 FROM channels WHERE id = $1",
                &[&id.0],
            )
            .map_err(|error| DomainError::Database(error.to_string()))?
            .ok_or(DomainError::NotFound(id.0))?;
        Ok(Self::map_row(&row))
    }

    fn find_all(&self) -> Result<Vec<Channel>, DomainError> {
        let mut client = self
            .pool
            .get()
            .map_err(|error| DomainError::Database(error.to_string()))?;
        let rows = client
            .query(
                "SELECT id, counting_station_id, name, description, external_datasource_id
                 FROM channels ORDER BY name ASC",
                &[],
            )
            .map_err(|error| DomainError::Database(error.to_string()))?;
        Ok(rows.iter().map(Self::map_row).collect())
    }

    fn find_by_counting_station_id(
        &self,
        station_id: value_objects::CountingStationId,
    ) -> Result<Vec<Channel>, DomainError> {
        let mut client = self
            .pool
            .get()
            .map_err(|error| DomainError::Database(error.to_string()))?;
        let rows = client
            .query(
                "SELECT id, counting_station_id, name, description, external_datasource_id
                 FROM channels WHERE counting_station_id = $1 ORDER BY name ASC",
                &[&station_id.0],
            )
            .map_err(|error| DomainError::Database(error.to_string()))?;
        Ok(rows.iter().map(Self::map_row).collect())
    }

    fn find_by_external_datasource_id(
        &self,
        external_id: value_objects::ExternalDatasourceId,
    ) -> Result<Option<Channel>, DomainError> {
        let mut client = self
            .pool
            .get()
            .map_err(|error| DomainError::Database(error.to_string()))?;
        let row = client
            .query_opt(
                "SELECT id, counting_station_id, name, description, external_datasource_id
                 FROM channels WHERE external_datasource_id = $1",
                &[&external_id.0],
            )
            .map_err(|error| DomainError::Database(error.to_string()))?;
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
            .map_err(|error| DomainError::Database(error.to_string()))?;
        let rows = match (counting_station_id, name) {
            (Some(station_id), Some(name)) => client
                .query(
                    "SELECT id, counting_station_id, name, description, external_datasource_id
                     FROM channels
                     WHERE counting_station_id = $1 AND name ILIKE $2
                     ORDER BY name ASC",
                    &[&station_id.0, &format!("%{}%", escape_like(name))],
                )
                .map_err(|error| DomainError::Database(error.to_string()))?,
            (Some(station_id), None) => client
                .query(
                    "SELECT id, counting_station_id, name, description, external_datasource_id
                     FROM channels WHERE counting_station_id = $1 ORDER BY name ASC",
                    &[&station_id.0],
                )
                .map_err(|error| DomainError::Database(error.to_string()))?,
            (None, Some(name)) => client
                .query(
                    "SELECT id, counting_station_id, name, description, external_datasource_id
                     FROM channels WHERE name ILIKE $1 ORDER BY name ASC",
                    &[&format!("%{}%", escape_like(name))],
                )
                .map_err(|error| DomainError::Database(error.to_string()))?,
            (None, None) => client
                .query(
                    "SELECT id, counting_station_id, name, description, external_datasource_id
                     FROM channels ORDER BY name ASC",
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
