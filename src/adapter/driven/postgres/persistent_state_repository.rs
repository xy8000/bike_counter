use std::collections::HashMap;

use crate::core::domain::data_source::data_source::value_objects::Id;
use crate::core::domain::data_source::persistent_state::PersistentStateStore;
use crate::core::domain::error::DomainError;

use super::pool::PgPool;

pub struct PostgresPersistentStateRepository {
    pool: PgPool,
}

impl PostgresPersistentStateRepository {
    pub fn new(pool: &PgPool) -> Self {
        Self { pool: pool.clone() }
    }
}

impl PersistentStateStore for PostgresPersistentStateRepository {
    fn get(&self, data_source_id: Id) -> Result<HashMap<String, String>, DomainError> {
        let mut client = self
            .pool
            .get()
            .map_err(|error| DomainError::Database(error.to_string()))?;
        let rows = client
            .query(
                "SELECT key, value FROM data_source_persistent_state WHERE data_source_id = $1",
                &[&data_source_id.0],
            )
            .map_err(|error| DomainError::Database(error.to_string()))?;
        Ok(rows
            .iter()
            .map(|row| (row.get::<_, String>(0), row.get::<_, String>(1)))
            .collect())
    }

    fn set(&self, data_source_id: Id, key: &str, value: &str) -> Result<(), DomainError> {
        let mut client = self
            .pool
            .get()
            .map_err(|error| DomainError::Database(error.to_string()))?;
        let id = uuid::Uuid::new_v4();
        client
            .execute(
                "INSERT INTO data_source_persistent_state (id, data_source_id, key, value)
                 VALUES ($1, $2, $3, $4)
                 ON CONFLICT ON CONSTRAINT uq_data_source_persistent_state_key
                 DO UPDATE SET value = $4, updated_at = now()",
                &[&id, &data_source_id.0, &key, &value],
            )
            .map_err(|error| DomainError::Database(error.to_string()))?;
        Ok(())
    }

    fn delete(&self, data_source_id: Id, key: &str) -> Result<(), DomainError> {
        let mut client = self
            .pool
            .get()
            .map_err(|error| DomainError::Database(error.to_string()))?;
        client
            .execute(
                "DELETE FROM data_source_persistent_state WHERE data_source_id = $1 AND key = $2",
                &[&data_source_id.0, &key],
            )
            .map_err(|error| DomainError::Database(error.to_string()))?;
        Ok(())
    }

    fn clear(&self, data_source_id: Id) -> Result<(), DomainError> {
        let mut client = self
            .pool
            .get()
            .map_err(|error| DomainError::Database(error.to_string()))?;
        client
            .execute(
                "DELETE FROM data_source_persistent_state WHERE data_source_id = $1",
                &[&data_source_id.0],
            )
            .map_err(|error| DomainError::Database(error.to_string()))?;
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use testcontainers::runners::SyncRunner;
    use testcontainers_modules::postgres::Postgres;

    use super::PostgresPersistentStateRepository;
    use crate::adapter::driven::postgres::PostgresDataSourceRepository;
    use crate::adapter::driven::postgres::create_pool;
    use crate::core::domain::configuration::configuration::value_objects::DatabaseConfiguration;
    use crate::core::domain::data_source::data_source::DataSource;
    use crate::core::domain::data_source::data_source::value_objects::Id;
    use crate::core::domain::data_source::persistent_state::PersistentStateStore;
    use crate::core::domain::data_source::repository::DataSourceRepository;

    /// A running Postgres test instance plus the repositories under test, so
    /// tests can also insert/update/delete data sources to exercise the FK
    /// cascade and the provider-change revoke trigger.
    struct TestDb {
        repository: PostgresPersistentStateRepository,
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
                repository: PostgresPersistentStateRepository::new(&pool),
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
    fn get_returns_empty_for_a_source_without_state() {
        let db = TestDb::new();
        let id = db.create_data_source("Münster");
        assert!(db.repository.get(id).unwrap().is_empty());
    }

    #[test]
    fn set_round_trips_and_overwrites() {
        let db = TestDb::new();
        let id = db.create_data_source("Münster");

        db.repository.set(id, "archive_checksum", "abc123").unwrap();
        db.repository
            .set(id, "archive_downloaded_at", "2026-01-01T00:00:00Z")
            .unwrap();

        let state = db.repository.get(id).unwrap();
        assert_eq!(state.len(), 2);
        assert_eq!(state.get("archive_checksum").unwrap(), "abc123");
        assert_eq!(
            state.get("archive_downloaded_at").unwrap(),
            "2026-01-01T00:00:00Z"
        );

        // Overwrite the same key (UNIQUE (data_source_id, key) upsert).
        db.repository.set(id, "archive_checksum", "def456").unwrap();
        let state = db.repository.get(id).unwrap();
        assert_eq!(state.len(), 2);
        assert_eq!(state.get("archive_checksum").unwrap(), "def456");
    }

    #[test]
    fn delete_removes_only_the_named_key() {
        let db = TestDb::new();
        let id = db.create_data_source("Münster");
        db.repository.set(id, "archive_checksum", "abc").unwrap();
        db.repository.set(id, "archive_file", "/tmp/x.zip").unwrap();

        db.repository.delete(id, "archive_checksum").unwrap();

        let state = db.repository.get(id).unwrap();
        assert_eq!(state.len(), 1);
        assert_eq!(state.get("archive_file").unwrap(), "/tmp/x.zip");
    }

    #[test]
    fn clear_wipes_the_whole_store() {
        let db = TestDb::new();
        let id = db.create_data_source("Münster");
        db.repository.set(id, "archive_checksum", "abc").unwrap();
        db.repository.set(id, "archive_file", "/tmp/x.zip").unwrap();

        db.repository.clear(id).unwrap();

        assert!(db.repository.get(id).unwrap().is_empty());
    }

    #[test]
    fn state_is_scoped_per_data_source() {
        let db = TestDb::new();
        let first = db.create_data_source("Münster");
        let second = db.create_data_source("Köln");

        db.repository.set(first, "archive_checksum", "abc").unwrap();

        assert!(db.repository.get(second).unwrap().is_empty());
        assert_eq!(db.repository.get(first).unwrap().len(), 1);
    }

    #[test]
    fn cascades_state_on_data_source_delete() {
        let db = TestDb::new();
        let id = db.create_data_source("Münster");
        db.repository.set(id, "archive_checksum", "abc").unwrap();

        db.data_source_repository.delete(id).unwrap();

        assert!(db.repository.get(id).unwrap().is_empty());
    }

    #[test]
    fn revokes_state_when_provider_type_changes() {
        let db = TestDb::new();
        let id = db.create_data_source("Münster");
        db.repository.set(id, "archive_checksum", "abc").unwrap();

        // Change the provider_type via the repository upsert; the AFTER UPDATE
        // OF provider_type trigger must drop the source's state rows.
        db.data_source_repository
            .upsert(DataSource::new(
                "Münster".to_string(),
                "some_other_provider".to_string(),
            ))
            .unwrap();

        assert!(db.repository.get(id).unwrap().is_empty());
    }
}
