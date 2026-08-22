use std::str::FromStr;
use std::sync::Mutex;

use chrono::{DateTime, Utc};
use postgres::{Client, Config as PostgresConfig, NoTls};
use refinery::embed_migrations;

use crate::core::domain::configuration::configuration::value_objects::DatabaseConfiguration;
use crate::core::domain::data_source::data_source::{DataSource, value_objects};
use crate::core::domain::data_source::repository::DataSourceRepository;
use crate::core::domain::error::DomainError;

embed_migrations!("migrations");

pub struct PostgresDataSourceRepository {
    client: Mutex<Client>,
}

impl PostgresDataSourceRepository {
    pub fn new(configuration: &DatabaseConfiguration) -> Result<Self, DomainError> {
        let mut config = PostgresConfig::from_str(configuration.database_url())
            .map_err(|error| DomainError::Database(error.to_string()))?;
        config.user(configuration.user());
        config.password(configuration.password());
        config.dbname(configuration.database_name());
        let mut client = config
            .connect(NoTls)
            .map_err(|error| DomainError::Database(error.to_string()))?;
        migrations::runner()
            .run(&mut client)
            .map_err(|error| DomainError::Database(error.to_string()))?;
        Ok(Self {
            client: Mutex::new(client),
        })
    }

    fn map_row(row: &postgres::Row) -> DataSource {
        DataSource {
            id: value_objects::Id(row.get(0)),
            name: value_objects::Name(row.get(1)),
            provider_type: value_objects::ProviderType(row.get(2)),
            last_updated_at: row.get(3),
        }
    }
}

impl DataSourceRepository for PostgresDataSourceRepository {
    fn upsert(&self, data_source: DataSource) -> Result<(), DomainError> {
        let mut client = self
            .client
            .lock()
            .map_err(|error| DomainError::Database(error.to_string()))?;
        client
            .execute(
                "INSERT INTO data_sources (id, name, provider_type) VALUES ($1, $2, $3)
                 ON CONFLICT (id) DO UPDATE SET name = $2, provider_type = $3",
                &[
                    &data_source.id.0,
                    &data_source.name.0,
                    &data_source.provider_type.0,
                ],
            )
            .map_err(|error| DomainError::Database(error.to_string()))?;
        Ok(())
    }

    fn find_by_id(&self, id: value_objects::Id) -> Result<Option<DataSource>, DomainError> {
        let mut client = self
            .client
            .lock()
            .map_err(|error| DomainError::Database(error.to_string()))?;
        let row = client
            .query_opt(
                "SELECT id, name, provider_type, last_updated_at FROM data_sources WHERE id = $1",
                &[&id.0],
            )
            .map_err(|error| DomainError::Database(error.to_string()))?;
        Ok(row.as_ref().map(Self::map_row))
    }

    fn find_by_name(&self, name: &str) -> Result<Option<DataSource>, DomainError> {
        let mut client = self
            .client
            .lock()
            .map_err(|error| DomainError::Database(error.to_string()))?;
        let row = client
            .query_opt(
                "SELECT id, name, provider_type, last_updated_at FROM data_sources WHERE name = $1",
                &[&name],
            )
            .map_err(|error| DomainError::Database(error.to_string()))?;
        Ok(row.as_ref().map(Self::map_row))
    }

    fn find_all(&self) -> Result<Vec<DataSource>, DomainError> {
        let mut client = self
            .client
            .lock()
            .map_err(|error| DomainError::Database(error.to_string()))?;
        let rows = client
            .query(
                "SELECT id, name, provider_type, last_updated_at FROM data_sources ORDER BY name ASC",
                &[],
            )
            .map_err(|error| DomainError::Database(error.to_string()))?;
        Ok(rows.iter().map(Self::map_row).collect())
    }

    fn delete(&self, id: value_objects::Id) -> Result<(), DomainError> {
        let mut client = self
            .client
            .lock()
            .map_err(|error| DomainError::Database(error.to_string()))?;
        client
            .execute("DELETE FROM data_sources WHERE id = $1", &[&id.0])
            .map_err(|error| DomainError::Database(error.to_string()))?;
        Ok(())
    }

    fn update_last_updated_at(
        &self,
        id: value_objects::Id,
        timestamp: DateTime<Utc>,
    ) -> Result<(), DomainError> {
        let mut client = self
            .client
            .lock()
            .map_err(|error| DomainError::Database(error.to_string()))?;
        client
            .execute(
                "UPDATE data_sources SET last_updated_at = $2 WHERE id = $1",
                &[&id.0, &timestamp],
            )
            .map_err(|error| DomainError::Database(error.to_string()))?;
        Ok(())
    }
}
