use std::str::FromStr;

use crate::core::domain::data_source::data_source::value_objects::Id;
use crate::core::domain::data_source::provider_message::{
    ProviderMessage, ProviderMessageSeverity, ProviderMessageStore,
};
use crate::core::domain::error::DomainError;

use super::pool::PgPool;

pub struct PostgresProviderMessageRepository {
    pool: PgPool,
}

impl PostgresProviderMessageRepository {
    pub fn new(pool: &PgPool) -> Self {
        Self { pool: pool.clone() }
    }
}

impl ProviderMessageStore for PostgresProviderMessageRepository {
    fn record(
        &self,
        data_source_id: Id,
        severity: ProviderMessageSeverity,
        message: &str,
    ) -> Result<(), DomainError> {
        let mut client = self
            .pool
            .get()
            .map_err(|error| DomainError::Database(error.to_string()))?;
        let id = uuid::Uuid::new_v4();
        let severity = severity.as_str();
        client
            .execute(
                "INSERT INTO data_source_provider_messages (id, data_source_id, severity, message)
                 VALUES ($1, $2, $3, $4)",
                &[&id, &data_source_id.0, &severity, &message],
            )
            .map_err(|error| DomainError::Database(error.to_string()))?;
        Ok(())
    }

    fn find_by_data_source(&self, data_source_id: Id) -> Result<Vec<ProviderMessage>, DomainError> {
        let mut client = self
            .pool
            .get()
            .map_err(|error| DomainError::Database(error.to_string()))?;
        let rows = client
            .query(
                "SELECT id, data_source_id, severity, message, occurred_at
                 FROM data_source_provider_messages
                 WHERE data_source_id = $1
                 ORDER BY occurred_at DESC, id DESC",
                &[&data_source_id.0],
            )
            .map_err(|error| DomainError::Database(error.to_string()))?;
        Ok(rows
            .iter()
            .map(|row| {
                let severity_raw: String = row.get(2);
                // The database CHECK constraint guarantees a known severity.
                let severity = ProviderMessageSeverity::from_str(&severity_raw)
                    .expect("database CHECK constraint guarantees a known severity");
                ProviderMessage {
                    id: row.get(0),
                    data_source_id: Id(row.get(1)),
                    severity,
                    message: row.get(3),
                    occurred_at: row.get::<_, chrono::DateTime<chrono::Utc>>(4),
                }
            })
            .collect())
    }
}

#[cfg(test)]
mod tests {
    use testcontainers::runners::SyncRunner;
    use testcontainers_modules::postgres::Postgres;

    use super::PostgresProviderMessageRepository;
    use crate::adapter::driven::postgres::PostgresDataSourceRepository;
    use crate::adapter::driven::postgres::create_pool;
    use crate::core::domain::configuration::configuration::value_objects::DatabaseConfiguration;
    use crate::core::domain::data_source::data_source::DataSource;
    use crate::core::domain::data_source::data_source::value_objects::Id;
    use crate::core::domain::data_source::provider_message::{
        ProviderMessageSeverity, ProviderMessageStore,
    };
    use crate::core::domain::data_source::repository::DataSourceRepository;

    /// A running Postgres test instance plus the repositories under test, so
    /// tests can also insert/delete data sources to exercise the FK cascade.
    struct TestDb {
        repository: PostgresProviderMessageRepository,
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
                repository: PostgresProviderMessageRepository::new(&pool),
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
    fn get_returns_empty_for_a_source_without_messages() {
        let db = TestDb::new();
        let id = db.create_data_source("Münster");
        assert!(db.repository.find_by_data_source(id).unwrap().is_empty());
    }

    #[test]
    fn record_round_trips_with_generated_occurred_at() {
        let db = TestDb::new();
        let id = db.create_data_source("Münster");

        db.repository
            .record(
                id,
                ProviderMessageSeverity::Warning,
                "channel 1 has no column",
            )
            .unwrap();

        let messages = db.repository.find_by_data_source(id).unwrap();
        assert_eq!(messages.len(), 1);
        assert_eq!(messages[0].severity, ProviderMessageSeverity::Warning);
        assert_eq!(messages[0].message, "channel 1 has no column");
        assert_eq!(messages[0].data_source_id, id);
        // occurred_at is generated by the database, not the caller.
        assert!(!messages[0].occurred_at.to_rfc3339().is_empty());
    }

    #[test]
    fn orders_newest_first() {
        let db = TestDb::new();
        let id = db.create_data_source("Münster");

        // Insert sequentially; identical severities are fine, ordering relies on
        // occurred_at (and id as a tie-breaker within the same timestamp).
        db.repository
            .record(id, ProviderMessageSeverity::Info, "first")
            .unwrap();
        db.repository
            .record(id, ProviderMessageSeverity::Warning, "second")
            .unwrap();

        let messages = db.repository.find_by_data_source(id).unwrap();
        assert_eq!(messages.len(), 2);
        assert_eq!(messages[0].message, "second");
        assert_eq!(messages[1].message, "first");
    }

    #[test]
    fn messages_are_scoped_per_data_source() {
        let db = TestDb::new();
        let first = db.create_data_source("Münster");
        let second = db.create_data_source("Köln");

        db.repository
            .record(first, ProviderMessageSeverity::Warning, "only for Münster")
            .unwrap();

        assert!(
            db.repository
                .find_by_data_source(second)
                .unwrap()
                .is_empty()
        );
        assert_eq!(db.repository.find_by_data_source(first).unwrap().len(), 1);
    }

    #[test]
    fn cascades_messages_on_data_source_delete() {
        let db = TestDb::new();
        let id = db.create_data_source("Münster");
        db.repository
            .record(id, ProviderMessageSeverity::Warning, "doomed")
            .unwrap();

        db.data_source_repository.delete(id).unwrap();

        assert!(db.repository.find_by_data_source(id).unwrap().is_empty());
    }
}
