//! Postgres-backed [`DataImportRunRepository`] implementation.
//!
//! The `data_source_imports` table stores one row per per-source import run so
//! the data-sources UI can show per-source last-import facts. Rows are written
//! by the data-source update service and read back newest-first per source.

use std::str::FromStr;

use chrono::{DateTime, Utc};
use uuid::Uuid;

use crate::core::domain::data_source::data_source::value_objects::Id as DataSourceId;
use crate::core::domain::data_source::import_run::{DataImportRun, ImportRunStatus};
use crate::core::domain::data_source::import_run_port::DataImportRunRepository;
use crate::core::domain::error::DomainError;

use super::pool::PgPool;

pub struct PostgresImportRunRepository {
    pool: PgPool,
}

impl PostgresImportRunRepository {
    pub fn new(pool: &PgPool) -> Self {
        Self { pool: pool.clone() }
    }

    fn map_row(row: &postgres::Row) -> Result<DataImportRun, DomainError> {
        let status: String = row.get("status");
        Ok(DataImportRun {
            id: row.get("id"),
            data_source_id: DataSourceId(row.get("data_source_id")),
            job_id: row.get("job_id"),
            started_at: row.get("started_at"),
            finished_at: row.get("finished_at"),
            status: ImportRunStatus::from_str(&status)?,
            failure_message: row.get("failure_message"),
        })
    }
}

impl DataImportRunRepository for PostgresImportRunRepository {
    fn insert(&self, run: &DataImportRun) -> Result<(), DomainError> {
        let mut client = self
            .pool
            .get()
            .map_err(|error| DomainError::Database(error.to_string()))?;
        client
            .execute(
                "INSERT INTO data_source_imports \
                 (id, data_source_id, job_id, started_at, finished_at, status, failure_message) \
                 VALUES ($1, $2, $3, $4, $5, $6, $7)",
                &[
                    &run.id,
                    &run.data_source_id.0,
                    &run.job_id,
                    &run.started_at,
                    &run.finished_at,
                    &run.status.as_str(),
                    &run.failure_message,
                ],
            )
            .map_err(|error| DomainError::Database(error.to_string()))?;
        Ok(())
    }

    fn finish(&self, id: Uuid, finished_at: DateTime<Utc>) -> Result<(), DomainError> {
        let mut client = self
            .pool
            .get()
            .map_err(|error| DomainError::Database(error.to_string()))?;
        let updated = client
            .execute(
                "UPDATE data_source_imports SET status = 'FINISHED', finished_at = $2, \
                 failure_message = NULL WHERE id = $1 AND status = 'RUNNING'",
                &[&id, &finished_at],
            )
            .map_err(|error| DomainError::Database(error.to_string()))?;
        if updated == 0 {
            return Err(DomainError::InvalidQuery(format!(
                "import run {id} is not RUNNING and cannot be finished"
            )));
        }
        Ok(())
    }

    fn fail(&self, id: Uuid, finished_at: DateTime<Utc>, message: &str) -> Result<(), DomainError> {
        let mut client = self
            .pool
            .get()
            .map_err(|error| DomainError::Database(error.to_string()))?;
        let updated = client
            .execute(
                "UPDATE data_source_imports SET status = 'FAILED', finished_at = $2, \
                 failure_message = $3 WHERE id = $1 AND status = 'RUNNING'",
                &[&id, &finished_at, &message],
            )
            .map_err(|error| DomainError::Database(error.to_string()))?;
        if updated == 0 {
            return Err(DomainError::InvalidQuery(format!(
                "import run {id} is not RUNNING and cannot be failed"
            )));
        }
        Ok(())
    }

    fn latest_by_data_source(
        &self,
        data_source_id: DataSourceId,
    ) -> Result<Option<DataImportRun>, DomainError> {
        let mut client = self
            .pool
            .get()
            .map_err(|error| DomainError::Database(error.to_string()))?;
        let row = client
            .query_opt(
                "SELECT id, data_source_id, job_id, started_at, finished_at, status, failure_message \
                 FROM data_source_imports \
                 WHERE data_source_id = $1 \
                 ORDER BY started_at DESC LIMIT 1",
                &[&data_source_id.0],
            )
            .map_err(|error| DomainError::Database(error.to_string()))?;
        row.map(|row| Self::map_row(&row)).transpose()
    }

    fn finalize_orphaned_running(&self, older_than: DateTime<Utc>) -> Result<u64, DomainError> {
        let mut client = self
            .pool
            .get()
            .map_err(|error| DomainError::Database(error.to_string()))?;
        // Job-linked orphans: a RUNNING run whose aggregate job already reached a
        // terminal state was abandoned by a worker that is gone or stuck (a
        // healthy worker finalizes its own run before the job goes terminal).
        // Finished-at mirrors the job's own finish time so the UI duration is
        // accurate.
        let job_linked = client
            .execute(
                "UPDATE data_source_imports AS dsi \
                 SET status = 'FINISHED', finished_at = COALESCE(j.finished_at, now()), \
                     failure_message = NULL \
                 FROM jobs j \
                 WHERE dsi.status = 'RUNNING' \
                   AND j.id = dsi.job_id \
                   AND j.status IN ('FINISHED', 'FAILED', 'CANCELLED')",
                &[],
            )
            .map_err(|error| DomainError::Database(error.to_string()))?;
        // Unlinked-orphan safety net: a RUNNING run without a job reference that
        // started before `older_than` can never be finalized by a live worker.
        let unlinked = client
            .execute(
                "UPDATE data_source_imports \
                 SET status = 'FINISHED', finished_at = now(), failure_message = NULL \
                 WHERE status = 'RUNNING' AND job_id IS NULL AND started_at < $1",
                &[&older_than],
            )
            .map_err(|error| DomainError::Database(error.to_string()))?;
        Ok(job_linked + unlinked)
    }
}

#[cfg(test)]
mod tests {
    use std::str::FromStr;

    use chrono::{DateTime, Duration, Utc};
    use postgres::{Config as PostgresConfig, NoTls};
    use testcontainers::runners::SyncRunner;
    use testcontainers_modules::postgres::Postgres;
    use uuid::Uuid;

    use super::PostgresImportRunRepository;
    use crate::adapter::driven::postgres::{PostgresDataSourceRepository, create_pool};
    use crate::core::domain::configuration::configuration::value_objects::DatabaseConfiguration;
    use crate::core::domain::data_source::data_source::DataSource;
    use crate::core::domain::data_source::data_source::value_objects::Id;
    use crate::core::domain::data_source::import_run::DataImportRun;
    use crate::core::domain::data_source::import_run_port::DataImportRunRepository;
    use crate::core::domain::data_source::repository_port::DataSourceRepository;

    /// A running Postgres test instance plus the repositories under test.
    struct TestDb {
        repository: PostgresImportRunRepository,
        data_source_repository: PostgresDataSourceRepository,
        url: String,
        user: String,
        password: String,
        dbname: String,
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
                url.clone(),
                database_user.to_string(),
                database_password.to_string(),
                database_name.to_string(),
            )
            .unwrap();
            let pool = create_pool(&configuration).unwrap();
            Self {
                repository: PostgresImportRunRepository::new(&pool),
                data_source_repository: PostgresDataSourceRepository::new(&pool),
                url,
                user: database_user.to_string(),
                password: database_password.to_string(),
                dbname: database_name.to_string(),
                _container: container,
            }
        }

        /// A raw synchronous connection for inserting `jobs` rows and asserting
        /// exact per-row state (the import-run repository has no find-by-id).
        fn raw_client(&self) -> postgres::Client {
            let mut config = PostgresConfig::from_str(&self.url).unwrap();
            config
                .user(&self.user)
                .password(&self.password)
                .dbname(&self.dbname);
            config.connect(NoTls).unwrap()
        }

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
    fn insert_finish_and_read_latest_round_trip() {
        let db = TestDb::new();
        let data_source_id = db.create_data_source("Münster");
        let now = DateTime::parse_from_rfc3339("2024-01-01T12:00:00Z")
            .unwrap()
            .with_timezone(&Utc);

        let run = DataImportRun::start(Uuid::new_v4(), data_source_id, None, now);
        db.repository.insert(&run).unwrap();

        db.repository
            .finish(run.id, now + Duration::seconds(90))
            .unwrap();

        let latest = db
            .repository
            .latest_by_data_source(data_source_id)
            .unwrap()
            .unwrap();
        assert_eq!(latest.id, run.id);
        assert_eq!(latest.status.as_str(), "FINISHED");
        assert!(latest.finished_at.is_some());
    }

    #[test]
    fn fail_sets_failure_message_and_latest_wins_by_started_at() {
        let db = TestDb::new();
        let data_source_id = db.create_data_source("Bonn");
        let earlier = DateTime::parse_from_rfc3339("2024-01-01T10:00:00Z")
            .unwrap()
            .with_timezone(&Utc);
        let later = DateTime::parse_from_rfc3339("2024-01-01T11:00:00Z")
            .unwrap()
            .with_timezone(&Utc);

        let first = DataImportRun::start(Uuid::new_v4(), data_source_id, None, earlier);
        db.repository.insert(&first).unwrap();
        db.repository
            .finish(first.id, earlier + Duration::seconds(30))
            .unwrap();

        let second = DataImportRun::start(Uuid::new_v4(), data_source_id, None, later);
        db.repository.insert(&second).unwrap();
        db.repository
            .fail(
                second.id,
                later + Duration::seconds(10),
                "provider unreachable",
            )
            .unwrap();

        let latest = db
            .repository
            .latest_by_data_source(data_source_id)
            .unwrap()
            .unwrap();
        assert_eq!(latest.id, second.id);
        assert_eq!(latest.status.as_str(), "FAILED");
        assert_eq!(
            latest.failure_message.as_deref(),
            Some("provider unreachable")
        );
    }

    #[test]
    fn finishing_a_non_running_run_is_an_error() {
        let db = TestDb::new();
        let data_source_id = db.create_data_source("Hamburg");
        let now = Utc::now();
        let run = DataImportRun::start(Uuid::new_v4(), data_source_id, None, now);
        db.repository.insert(&run).unwrap();
        db.repository
            .finish(run.id, now + Duration::seconds(1))
            .unwrap();
        assert!(
            db.repository
                .finish(run.id, now + Duration::seconds(2))
                .is_err()
        );
    }

    /// Inserts a minimal `jobs` row (a `data_source_update` job) and returns its id.
    fn insert_job(client: &mut postgres::Client, status: &str) -> Uuid {
        let id = Uuid::new_v4();
        client
            .execute(
                "INSERT INTO jobs \
                 (id, name, job_type, status, started_at, finished_at, failure_message, metadata) \
                 VALUES ($1, 'data source update', 'data_source_update', $2, now(), now(), NULL, \
                         '{}'::jsonb)",
                &[&id, &status],
            )
            .unwrap();
        id
    }

    #[test]
    fn finalize_orphaned_running_reaps_terminal_and_unlinked_orphans() {
        let db = TestDb::new();
        let source_a = db.create_data_source("Bonn");
        let source_b = db.create_data_source("Münster");
        let source_c = db.create_data_source("Hamburg");

        let old = DateTime::parse_from_rfc3339("2024-01-01T00:00:00Z")
            .unwrap()
            .with_timezone(&Utc);
        let recent = DateTime::parse_from_rfc3339("2024-06-01T00:00:00Z")
            .unwrap()
            .with_timezone(&Utc);
        // The unlinked-orphan grace boundary (everything before it is stale).
        let older_than = DateTime::parse_from_rfc3339("2024-03-01T00:00:00Z")
            .unwrap()
            .with_timezone(&Utc);

        let mut client = db.raw_client();
        let finished_job = insert_job(&mut client, "FINISHED");
        let running_job = insert_job(&mut client, "RUNNING");
        drop(client);

        // Orphan 1: RUNNING but its aggregate job is FINISHED -> reaped.
        let linked_orphan = DataImportRun::start(Uuid::new_v4(), source_a, Some(finished_job), old);
        db.repository.insert(&linked_orphan).unwrap();
        // Spared: RUNNING under a still-RUNNING job.
        let linked_live = DataImportRun::start(Uuid::new_v4(), source_b, Some(running_job), old);
        db.repository.insert(&linked_live).unwrap();
        // Orphan 2: unlinked and older than the grace window -> reaped.
        let unlinked_orphan = DataImportRun::start(Uuid::new_v4(), source_c, None, old);
        db.repository.insert(&unlinked_orphan).unwrap();
        // Spared: unlinked but started within the grace window.
        let unlinked_recent = DataImportRun::start(Uuid::new_v4(), source_c, None, recent);
        db.repository.insert(&unlinked_recent).unwrap();

        let reaped = db.repository.finalize_orphaned_running(older_than).unwrap();
        assert_eq!(reaped, 2, "job-terminal + stale unlinked runs are reaped");

        let mut client = db.raw_client();
        for (id, expected) in [
            (linked_orphan.id, "FINISHED"),
            (unlinked_orphan.id, "FINISHED"),
            (linked_live.id, "RUNNING"),
            (unlinked_recent.id, "RUNNING"),
        ] {
            let status: String = client
                .query_one(
                    "SELECT status FROM data_source_imports WHERE id = $1",
                    &[&id],
                )
                .unwrap()
                .get(0);
            assert_eq!(status, expected, "run {id} must stay {expected}");
        }
    }
}
