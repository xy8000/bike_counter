use crate::core::domain::counting_stations::counting_station::{CountingStation, value_objects};
use crate::core::domain::counting_stations::repository::CountingStationRepository;
use crate::core::domain::error::DomainError;

use super::postgres_pool::PgPool;

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
                "INSERT INTO counting_stations (id, name, description, external_datasource_id, data_source_id)
                 VALUES ($1, $2, $3, $4, $5)",
                &[
                    &station.id.0,
                    &station.name.0,
                    &station.description.0,
                    &station.external_datasource_id.as_ref().map(|id| id.0.as_str()),
                    &station.data_source_id.map(|id| id.0),
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
                "SELECT id, name, description, external_datasource_id, data_source_id
                 FROM counting_stations WHERE id = $1",
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
                "SELECT id, name, description, external_datasource_id, data_source_id
                 FROM counting_stations ORDER BY name ASC",
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
                "SELECT id, name, description, external_datasource_id, data_source_id
                 FROM counting_stations WHERE external_datasource_id = $1",
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
                    "SELECT id, name, description, external_datasource_id, data_source_id
                     FROM counting_stations WHERE name ILIKE $1 ORDER BY name ASC",
                    &[&format!("%{}%", escape_like(name))],
                )
                .map_err(|error| DomainError::Database(error.to_string()))?,
            None => client
                .query(
                    "SELECT id, name, description, external_datasource_id, data_source_id
                     FROM counting_stations ORDER BY name ASC",
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
