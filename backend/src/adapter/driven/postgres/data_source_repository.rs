use chrono::{DateTime, Utc};

use crate::core::domain::data_source::data_source::{DataSource, value_objects};
use crate::core::domain::data_source::repository_port::DataSourceRepository;
use crate::core::domain::error::DomainError;

use super::pool::PgPool;

pub struct PostgresDataSourceRepository {
    pool: PgPool,
}

impl PostgresDataSourceRepository {
    pub fn new(pool: &PgPool) -> Self {
        Self { pool: pool.clone() }
    }

    fn map_row(row: &postgres::Row) -> DataSource {
        DataSource {
            id: value_objects::Id(row.get(0)),
            name: value_objects::Name(row.get(1)),
            provider_type: value_objects::ProviderType(row.get(2)),
            imported_until: row.get(3),
            last_updated_at: row.get(4),
        }
    }
}

impl DataSourceRepository for PostgresDataSourceRepository {
    fn upsert(&self, data_source: DataSource) -> Result<(), DomainError> {
        let mut client = self
            .pool
            .get()
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
            .pool
            .get()
            .map_err(|error| DomainError::Database(error.to_string()))?;
        let row = client
            .query_opt(
                "SELECT id, name, provider_type, imported_until, last_updated_at FROM data_sources WHERE id = $1",
                &[&id.0],
            )
            .map_err(|error| DomainError::Database(error.to_string()))?;
        Ok(row.as_ref().map(Self::map_row))
    }

    fn find_by_name(&self, name: &str) -> Result<Option<DataSource>, DomainError> {
        let mut client = self
            .pool
            .get()
            .map_err(|error| DomainError::Database(error.to_string()))?;
        let row = client
            .query_opt(
                "SELECT id, name, provider_type, imported_until, last_updated_at FROM data_sources WHERE name = $1",
                &[&name],
            )
            .map_err(|error| DomainError::Database(error.to_string()))?;
        Ok(row.as_ref().map(Self::map_row))
    }

    fn find_all(&self) -> Result<Vec<DataSource>, DomainError> {
        let mut client = self
            .pool
            .get()
            .map_err(|error| DomainError::Database(error.to_string()))?;
        let rows = client
            .query(
                "SELECT id, name, provider_type, imported_until, last_updated_at FROM data_sources ORDER BY name ASC",
                &[],
            )
            .map_err(|error| DomainError::Database(error.to_string()))?;
        Ok(rows.iter().map(Self::map_row).collect())
    }

    fn delete(&self, id: value_objects::Id) -> Result<(), DomainError> {
        let mut client = self
            .pool
            .get()
            .map_err(|error| DomainError::Database(error.to_string()))?;
        client
            .execute("DELETE FROM data_sources WHERE id = $1", &[&id.0])
            .map_err(|error| DomainError::Database(error.to_string()))?;
        Ok(())
    }

    fn update_imported_until(
        &self,
        id: value_objects::Id,
        timestamp: DateTime<Utc>,
    ) -> Result<(), DomainError> {
        let mut client = self
            .pool
            .get()
            .map_err(|error| DomainError::Database(error.to_string()))?;
        client
            .execute(
                "UPDATE data_sources SET imported_until = $2 WHERE id = $1",
                &[&id.0, &timestamp],
            )
            .map_err(|error| DomainError::Database(error.to_string()))?;
        Ok(())
    }

    fn clear_imported_until(&self, id: value_objects::Id) -> Result<(), DomainError> {
        let mut client = self
            .pool
            .get()
            .map_err(|error| DomainError::Database(error.to_string()))?;
        client
            .execute(
                "UPDATE data_sources SET imported_until = NULL WHERE id = $1",
                &[&id.0],
            )
            .map_err(|error| DomainError::Database(error.to_string()))?;
        Ok(())
    }

    fn update_last_updated(
        &self,
        id: value_objects::Id,
        timestamp: DateTime<Utc>,
    ) -> Result<(), DomainError> {
        let mut client = self
            .pool
            .get()
            .map_err(|error| DomainError::Database(error.to_string()))?;
        client
            .execute(
                "UPDATE data_sources SET last_updated_at = $2 WHERE id = $1",
                &[&id.0, &timestamp],
            )
            .map_err(|error| DomainError::Database(format!("{error:?}")))?;
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use chrono::{DateTime, Utc};
    use testcontainers::runners::SyncRunner;
    use testcontainers_modules::postgres::Postgres;

    use super::PostgresDataSourceRepository;
    use crate::adapter::driven::postgres::create_pool;
    use crate::core::domain::configuration::configuration::value_objects::DatabaseConfiguration;
    use crate::core::domain::data_source::data_source::DataSource;
    use crate::core::domain::data_source::data_source::value_objects::Id;
    use crate::core::domain::data_source::repository_port::DataSourceRepository;

    /// A running Postgres test instance plus the repository under test.
    struct TestDb {
        repository: PostgresDataSourceRepository,
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
                repository: PostgresDataSourceRepository::new(&pool),
                _container: container,
            }
        }
    }

    #[test]
    fn update_imported_until_and_clear_round_trip() {
        let db = TestDb::new();
        let data_source = DataSource::new(
            "Münster".to_string(),
            "münster_opendata_github_provider".to_string(),
        );
        db.repository.upsert(data_source.clone()).unwrap();

        let id: Id = data_source.id;
        let t0 = DateTime::parse_from_rfc3339("2024-01-01T00:00:00Z")
            .unwrap()
            .with_timezone(&Utc);
        db.repository.update_imported_until(id, t0).unwrap();

        let stored = db.repository.find_by_id(id).unwrap().unwrap();
        assert_eq!(stored.imported_until, Some(t0));

        db.repository.clear_imported_until(id).unwrap();
        let cleared = db.repository.find_by_id(id).unwrap().unwrap();
        assert_eq!(cleared.imported_until, None);
    }

    #[test]
    fn update_last_updated_round_trip() {
        let db = TestDb::new();
        let data_source = DataSource::new(
            "Münster".to_string(),
            "münster_opendata_github_provider".to_string(),
        );
        db.repository.upsert(data_source.clone()).unwrap();

        let id: Id = data_source.id;
        let t0 = DateTime::parse_from_rfc3339("2024-01-01T12:00:00Z")
            .unwrap()
            .with_timezone(&Utc);

        let before = db.repository.find_by_id(id).unwrap().unwrap();
        assert_eq!(before.last_updated_at, None);

        db.repository.update_last_updated(id, t0).unwrap();
        let stored = db.repository.find_by_id(id).unwrap().unwrap();
        // t0 has no fractional seconds, so it round-trips exactly through the
        // microsecond-precision TIMESTAMPTZ column.
        assert_eq!(stored.last_updated_at, Some(t0));
    }
}
