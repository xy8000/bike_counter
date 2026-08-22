//! Postgres-backed [`JobRepository`] implementation (ShedLock-style job table).
//!
//! `lifetime_until` is an absolute deadline timestamp (`TIMESTAMPTZ`) — a
//! RUNNING job only "lives" before this timestamp, matching the domain model.
//! Metadata is a generic JSONB map. The synchronous `postgres` client must only
//! be used from a blocking context (`spawn_blocking`), matching the other
//! driven repositories.

use std::str::FromStr;

use chrono::{DateTime, Utc};
use serde_json::Value;
use uuid::Uuid;

use crate::core::domain::error::DomainError;
use crate::core::domain::jobs::job::{Job, JobStatus};
use crate::core::domain::jobs::repository::JobRepository;

use super::postgres_pool::PgPool;

const SELECT_COLUMNS: &str = "id, name, job_type, status, started_at, finished_at, \
                              failure_message, metadata, lifetime_until, max_lifetime_exceeded";

pub struct PostgresJobRepository {
    pool: PgPool,
}

impl PostgresJobRepository {
    pub fn new(pool: &PgPool) -> Self {
        Self { pool: pool.clone() }
    }

    fn map_row(row: &postgres::Row) -> Result<Job, DomainError> {
        let metadata: Value = row.get("metadata");
        let status: String = row.get("status");
        Ok(Job {
            id: row.get("id"),
            name: row.get("name"),
            job_type: row.get("job_type"),
            status: JobStatus::from_str(&status)?,
            started_at: row.get("started_at"),
            finished_at: row.get("finished_at"),
            failure_message: row.get("failure_message"),
            metadata: metadata.as_object().cloned().unwrap_or_default(),
            lifetime_until: row.get("lifetime_until"),
            max_lifetime_exceeded: row.get("max_lifetime_exceeded"),
        })
    }
}

impl JobRepository for PostgresJobRepository {
    fn insert(&self, job: Job) -> Result<(), DomainError> {
        if job.lifetime_until <= Utc::now() {
            return Err(DomainError::InvalidQuery(
                "a job requires a lifetime_until deadline in the future".to_string(),
            ));
        }

        let mut client = self
            .pool
            .get()
            .map_err(|error| DomainError::Database(error.to_string()))?;
        let metadata = Value::Object(job.metadata);
        client
            .execute(
                "INSERT INTO jobs (id, name, job_type, status, metadata, lifetime_until, max_lifetime_exceeded) \
                 VALUES ($1, $2, $3, $4, $5, $6, $7)",
                &[
                    &job.id,
                    &job.name,
                    &job.job_type,
                    &job.status.as_str(),
                    &metadata,
                    &job.lifetime_until,
                    &job.max_lifetime_exceeded,
                ],
            )
            .map_err(|error| DomainError::Database(format!("{error:?}")))?;
        Ok(())
    }

    fn set_running(&self, id: Uuid, started_at: DateTime<Utc>) -> Result<(), DomainError> {
        let mut client = self
            .pool
            .get()
            .map_err(|error| DomainError::Database(error.to_string()))?;
        let updated = client
            .execute(
                "UPDATE jobs SET status = 'RUNNING', started_at = $2 \
                 WHERE id = $1 AND status = 'PENDING'",
                &[&id, &started_at],
            )
            .map_err(|error| DomainError::Database(error.to_string()))?;
        if updated == 0 {
            return Err(DomainError::InvalidQuery(format!(
                "job {id} is not in PENDING state and cannot be started"
            )));
        }
        Ok(())
    }

    fn set_finished(&self, id: Uuid, finished_at: DateTime<Utc>) -> Result<(), DomainError> {
        let mut client = self
            .pool
            .get()
            .map_err(|error| DomainError::Database(error.to_string()))?;
        let updated = client
            .execute(
                "UPDATE jobs SET status = 'FINISHED', finished_at = $2, failure_message = NULL \
                 WHERE id = $1 AND status = 'RUNNING'",
                &[&id, &finished_at],
            )
            .map_err(|error| DomainError::Database(error.to_string()))?;
        if updated == 0 {
            return Err(DomainError::InvalidQuery(format!(
                "job {id} is not in RUNNING state and cannot be finished"
            )));
        }
        Ok(())
    }

    fn set_failed(
        &self,
        id: Uuid,
        finished_at: DateTime<Utc>,
        message: &str,
    ) -> Result<(), DomainError> {
        let mut client = self
            .pool
            .get()
            .map_err(|error| DomainError::Database(error.to_string()))?;
        let updated = client
            .execute(
                "UPDATE jobs SET status = 'FAILED', finished_at = $2, failure_message = $3 \
                 WHERE id = $1 AND status IN ('PENDING', 'RUNNING')",
                &[&id, &finished_at, &message],
            )
            .map_err(|error| DomainError::Database(error.to_string()))?;
        if updated == 0 {
            return Err(DomainError::InvalidQuery(format!(
                "job {id} can only fail from PENDING or RUNNING state"
            )));
        }
        Ok(())
    }

    fn update_metadata(&self, id: Uuid, key: &str, value: Value) -> Result<(), DomainError> {
        let mut client = self
            .pool
            .get()
            .map_err(|error| DomainError::Database(error.to_string()))?;
        client
            .execute(
                "UPDATE jobs SET metadata = jsonb_set(metadata, ARRAY[$2], $3) WHERE id = $1",
                &[&id, &key, &value],
            )
            .map_err(|error| DomainError::Database(format!("{error:?}")))?;
        Ok(())
    }

    fn find_by_id(&self, id: Uuid) -> Result<Option<Job>, DomainError> {
        let mut client = self
            .pool
            .get()
            .map_err(|error| DomainError::Database(error.to_string()))?;
        let row = client
            .query_opt(
                &format!("SELECT {SELECT_COLUMNS} FROM jobs WHERE id = $1"),
                &[&id],
            )
            .map_err(|error| DomainError::Database(error.to_string()))?;
        row.map(|row| Self::map_row(&row)).transpose()
    }

    fn find_all(
        &self,
        job_type: Option<&str>,
        status: Option<JobStatus>,
    ) -> Result<Vec<Job>, DomainError> {
        let mut client = self
            .pool
            .get()
            .map_err(|error| DomainError::Database(error.to_string()))?;
        // Filters are pushed down to SQL so the existing idx_jobs_type_status
        // index can be used instead of loading all rows and filtering in Rust.
        // `Option` params bind to NULL when absent, keeping the WHERE clause
        // null-safe without building dynamic SQL.
        let query = format!(
            "SELECT {SELECT_COLUMNS} FROM jobs \
             WHERE ($1::text IS NULL OR job_type = $1) \
               AND ($2::text IS NULL OR status = $2) \
             ORDER BY created_at DESC"
        );
        let rows = client
            .query(&query, &[&job_type, &status.map(|status| status.as_str())])
            .map_err(|error| DomainError::Database(error.to_string()))?;
        rows.iter().map(Self::map_row).collect()
    }

    fn find_running_by_type(&self, job_type: &str) -> Result<Option<Job>, DomainError> {
        let mut client = self
            .pool
            .get()
            .map_err(|error| DomainError::Database(error.to_string()))?;
        let row = client
            .query_opt(
                &format!(
                    "SELECT {SELECT_COLUMNS} FROM jobs \
                     WHERE job_type = $1 AND status = 'RUNNING' ORDER BY created_at DESC LIMIT 1"
                ),
                &[&job_type],
            )
            .map_err(|error| DomainError::Database(error.to_string()))?;
        row.map(|row| Self::map_row(&row)).transpose()
    }

    fn find_last_finished_by_type(&self, job_type: &str) -> Result<Option<Job>, DomainError> {
        let mut client = self
            .pool
            .get()
            .map_err(|error| DomainError::Database(error.to_string()))?;
        let row = client
            .query_opt(
                &format!(
                    "SELECT {SELECT_COLUMNS} FROM jobs \
                     WHERE job_type = $1 AND status = 'FINISHED' ORDER BY created_at DESC LIMIT 1"
                ),
                &[&job_type],
            )
            .map_err(|error| DomainError::Database(error.to_string()))?;
        row.map(|row| Self::map_row(&row)).transpose()
    }

    fn expire_running_jobs(&self, job_type: &str, now: DateTime<Utc>) -> Result<u64, DomainError> {
        let mut client = self
            .pool
            .get()
            .map_err(|error| DomainError::Database(error.to_string()))?;
        let updated = client
            .execute(
                "UPDATE jobs SET status = 'FAILED', finished_at = $2, \
                 failure_message = 'Max lifetime exceeded', max_lifetime_exceeded = TRUE \
                 WHERE job_type = $1 AND status = 'RUNNING' AND lifetime_until < $2",
                &[&job_type, &now],
            )
            .map_err(|error| DomainError::Database(format!("{error:?}")))?;
        Ok(updated)
    }
}

#[cfg(test)]
mod tests {
    use std::str::FromStr;

    use chrono::{DateTime, Duration, Timelike, Utc};
    use postgres::{Config as PostgresConfig, NoTls};
    use serde_json::json;
    use testcontainers::runners::SyncRunner;
    use testcontainers_modules::postgres::Postgres;
    use uuid::Uuid;

    use super::PostgresJobRepository;
    use crate::adapter::driven::postgres_pool::create_pool;
    use crate::core::domain::configuration::configuration::value_objects::DatabaseConfiguration;
    use crate::core::domain::jobs::job::{Job, JobStatus};
    use crate::core::domain::jobs::repository::JobRepository;

    /// A running Postgres test instance plus its repository and connection
    /// details, so tests can also open a raw client to manipulate rows.
    struct TestDb {
        repository: PostgresJobRepository,
        // Keep the container handle alive for the lifetime of the test:
        // dropping it would stop the container and close the connection.
        _container: testcontainers::Container<Postgres>,
        url: String,
        user: String,
        password: String,
        dbname: String,
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
            let repository = PostgresJobRepository::new(&pool);
            Self {
                repository,
                _container: container,
                url,
                user: database_user.to_string(),
                password: database_password.to_string(),
                dbname: database_name.to_string(),
            }
        }

        fn raw_client(&self) -> postgres::Client {
            let mut config = PostgresConfig::from_str(&self.url).unwrap();
            config
                .user(&self.user)
                .password(&self.password)
                .dbname(&self.dbname);
            config.connect(NoTls).unwrap()
        }
    }

    fn job(id: Uuid, job_type: &str) -> Job {
        job_with_deadline(id, job_type, Utc::now() + Duration::seconds(3600))
    }

    fn job_with_deadline(id: Uuid, job_type: &str, deadline: DateTime<Utc>) -> Job {
        Job::new(
            id,
            format!("Job {job_type}"),
            job_type.to_string(),
            deadline,
        )
    }

    /// Postgres TIMESTAMPTZ stores microseconds; `chrono` keeps nanoseconds.
    /// Truncate so round-trip equality checks pass.
    fn micros(dt: DateTime<Utc>) -> DateTime<Utc> {
        dt - Duration::nanoseconds((dt.nanosecond() % 1000) as i64)
    }

    #[test]
    fn inserts_and_reads_job_with_lifetime_deadline_round_trip() {
        let db = TestDb::new();
        let id = Uuid::new_v4();
        let deadline = Utc::now() + Duration::seconds(3600);
        db.repository
            .insert(job_with_deadline(id, "data_source_update", deadline))
            .unwrap();

        let stored = db.repository.find_by_id(id).unwrap().unwrap();
        assert_eq!(stored.name, "Job data_source_update");
        assert_eq!(stored.status, JobStatus::Pending);
        assert_eq!(stored.lifetime_until, micros(deadline));
        assert!(!stored.max_lifetime_exceeded);
        assert!(stored.metadata.is_empty());
    }

    #[test]
    fn insert_with_past_deadline_fails() {
        let db = TestDb::new();
        let new_job = job_with_deadline(
            Uuid::new_v4(),
            "data_source_update",
            Utc::now() - Duration::seconds(60),
        );
        assert!(db.repository.insert(new_job).is_err());
    }

    #[test]
    fn runs_lifecycle_pending_to_running_to_finished() {
        let db = TestDb::new();
        let id = Uuid::new_v4();
        db.repository.insert(job(id, "data_source_update")).unwrap();

        let started = Utc::now();
        db.repository.set_running(id, started).unwrap();
        let running = db.repository.find_by_id(id).unwrap().unwrap();
        assert_eq!(running.status, JobStatus::Running);
        assert_eq!(running.started_at, Some(micros(started)));

        let finished = Utc::now();
        db.repository.set_finished(id, finished).unwrap();
        let done = db.repository.find_by_id(id).unwrap().unwrap();
        assert_eq!(done.status, JobStatus::Finished);
        assert_eq!(done.finished_at, Some(micros(finished)));
    }

    #[test]
    fn set_failed_allowed_from_pending_and_running() {
        let db = TestDb::new();
        let pending_id = Uuid::new_v4();
        db.repository.insert(job(pending_id, "t")).unwrap();
        db.repository
            .set_failed(pending_id, Utc::now(), "cancelled")
            .unwrap();

        let running_id = Uuid::new_v4();
        db.repository.insert(job(running_id, "t")).unwrap();
        db.repository.set_running(running_id, Utc::now()).unwrap();
        db.repository
            .set_failed(running_id, Utc::now(), "provider error")
            .unwrap();

        for id in [pending_id, running_id] {
            let stored = db.repository.find_by_id(id).unwrap().unwrap();
            assert_eq!(stored.status, JobStatus::Failed);
            assert!(stored.failure_message.is_some());
        }
    }

    #[test]
    fn updates_metadata_in_place() {
        let db = TestDb::new();
        let id = Uuid::new_v4();
        db.repository.insert(job(id, "data_source_update")).unwrap();

        db.repository
            .update_metadata(id, "processed_measurements", json!(1200))
            .unwrap();
        db.repository
            .update_metadata(id, "processed_measurements", json!(2400))
            .unwrap();

        let stored = db.repository.find_by_id(id).unwrap().unwrap();
        assert_eq!(
            stored.metadata.get("processed_measurements"),
            Some(&json!(2400))
        );
    }

    #[test]
    fn find_all_filters_by_type_and_status() {
        let db = TestDb::new();
        let running_id = Uuid::new_v4();
        db.repository
            .insert(job(running_id, "data_source_update"))
            .unwrap();
        db.repository.set_running(running_id, Utc::now()).unwrap();

        let finished_id = Uuid::new_v4();
        db.repository
            .insert(job(finished_id, "data_source_update"))
            .unwrap();
        db.repository.set_running(finished_id, Utc::now()).unwrap();
        db.repository.set_finished(finished_id, Utc::now()).unwrap();

        let other_id = Uuid::new_v4();
        db.repository.insert(job(other_id, "other")).unwrap();

        let all = db.repository.find_all(None, None).unwrap();
        assert_eq!(all.len(), 3);

        let by_type = db
            .repository
            .find_all(Some("data_source_update"), None)
            .unwrap();
        assert_eq!(by_type.len(), 2);

        let running = db
            .repository
            .find_all(Some("data_source_update"), Some(JobStatus::Running))
            .unwrap();
        assert_eq!(running.len(), 1);
        assert_eq!(running[0].id, running_id);

        assert_eq!(
            db.repository
                .find_running_by_type("data_source_update")
                .unwrap()
                .unwrap()
                .id,
            running_id
        );
        assert_eq!(
            db.repository
                .find_last_finished_by_type("data_source_update")
                .unwrap()
                .unwrap()
                .id,
            finished_id
        );
    }

    #[test]
    fn expire_running_jobs_flips_only_expired_running_jobs() {
        let db = TestDb::new();
        let now = Utc::now();

        // A RUNNING job whose deadline has passed: "no longer living".
        let expired_id = Uuid::new_v4();
        db.repository
            .insert(job_with_deadline(
                expired_id,
                "data_source_update",
                now + Duration::seconds(3600),
            ))
            .unwrap();
        db.repository
            .set_running(expired_id, now - Duration::seconds(7200))
            .unwrap();
        let mut client = db.raw_client();
        client
            .execute(
                "UPDATE jobs SET lifetime_until = $2 WHERE id = $1",
                &[&expired_id, &(now - Duration::seconds(1))],
            )
            .unwrap();
        drop(client);

        // A RUNNING job still within its deadline: still "living".
        let within_id = Uuid::new_v4();
        db.repository
            .insert(job_with_deadline(
                within_id,
                "data_source_update",
                now + Duration::seconds(3600),
            ))
            .unwrap();
        db.repository
            .set_running(within_id, now - Duration::seconds(600))
            .unwrap();

        // An expired RUNNING job of another type must not be touched.
        let expired_other = Uuid::new_v4();
        db.repository
            .insert(job_with_deadline(
                expired_other,
                "other",
                now + Duration::seconds(3600),
            ))
            .unwrap();
        db.repository
            .set_running(expired_other, now - Duration::seconds(7200))
            .unwrap();
        let mut client = db.raw_client();
        client
            .execute(
                "UPDATE jobs SET lifetime_until = $2 WHERE id = $1",
                &[&expired_other, &(now - Duration::seconds(1))],
            )
            .unwrap();
        drop(client);

        let expired = db
            .repository
            .expire_running_jobs("data_source_update", now)
            .unwrap();
        assert_eq!(expired, 1);

        let expired_job = db.repository.find_by_id(expired_id).unwrap().unwrap();
        assert_eq!(expired_job.status, JobStatus::Failed);
        assert!(expired_job.max_lifetime_exceeded);
        assert!(expired_job.failure_message.is_some());

        let within = db.repository.find_by_id(within_id).unwrap().unwrap();
        assert_eq!(within.status, JobStatus::Running);
        assert!(!within.max_lifetime_exceeded);

        // Jobs of other types are untouched.
        let other = db.repository.find_by_id(expired_other).unwrap().unwrap();
        assert_eq!(other.status, JobStatus::Running);
    }
}
