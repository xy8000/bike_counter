//! Postgres-backed [`JobRepository`] implementation.
//!
//! Jobs are tracked in the `jobs` table (history + status + ownership/liveness)
//! while mutual exclusion lives in the ShedLock-style `job_locks` table (unique
//! `job_type`, `locked_by`, `lock_until`). The only racy step is [`acquire`]:
//! a single atomic `INSERT ... ON CONFLICT (job_type) DO UPDATE ... WHERE
//! lock_until < now()`. A job row is only created after its lock is acquired and
//! is inserted directly as RUNNING with its `instance_id` + `heartbeat_at`. The
//! synchronous `postgres` client must only be used from a blocking context
//! (`spawn_blocking`), matching the other driven repositories.

use std::str::FromStr;

use chrono::{DateTime, Utc};
use serde_json::Value;
use uuid::Uuid;

use crate::core::domain::error::DomainError;
use crate::core::domain::jobs::job::{Job, JobStatus};
use crate::core::domain::jobs::repository_port::JobRepository;

use super::pool::PgPool;

const SELECT_COLUMNS: &str = "id, name, job_type, status, started_at, finished_at, \
                              failure_message, metadata, instance_id, heartbeat_at";

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
            instance_id: row.get("instance_id"),
            heartbeat_at: row.get("heartbeat_at"),
        })
    }
}

impl JobRepository for PostgresJobRepository {
    fn insert(&self, job: Job) -> Result<(), DomainError> {
        let mut client = self
            .pool
            .get()
            .map_err(|error| DomainError::Database(error.to_string()))?;
        let metadata = Value::Object(job.metadata);
        client
            .execute(
                "INSERT INTO jobs (id, name, job_type, status, started_at, finished_at, \
                                   failure_message, metadata, instance_id, heartbeat_at) \
                 VALUES ($1, $2, $3, $4, $5, $6, $7, $8, $9, $10)",
                &[
                    &job.id,
                    &job.name,
                    &job.job_type,
                    &job.status.as_str(),
                    &job.started_at,
                    &job.finished_at,
                    &job.failure_message,
                    &metadata,
                    &job.instance_id,
                    &job.heartbeat_at,
                ],
            )
            .map_err(|error| DomainError::Database(format!("{error:?}")))?;
        Ok(())
    }

    fn acquire(
        &self,
        job_type: &str,
        instance_id: Uuid,
        lock_until: DateTime<Utc>,
    ) -> Result<bool, DomainError> {
        let mut client = self
            .pool
            .get()
            .map_err(|error| DomainError::Database(error.to_string()))?;
        // ShedLock acquire: INSERT wins immediately; an existing lock row is only
        // taken over when its lock_until has expired. `updated == 1` means we
        // now own the type's lock.
        let updated = client
            .execute(
                "INSERT INTO job_locks (job_type, locked_by, lock_until) \
                 VALUES ($1, $2, $3) \
                 ON CONFLICT (job_type) DO UPDATE \
                   SET locked_by = EXCLUDED.locked_by, lock_until = EXCLUDED.lock_until \
                   WHERE job_locks.lock_until < now()",
                &[&job_type, &instance_id, &lock_until],
            )
            .map_err(|error| DomainError::Database(format!("{error:?}")))?;
        Ok(updated == 1)
    }

    fn release(&self, job_type: &str, instance_id: Uuid) -> Result<(), DomainError> {
        let mut client = self
            .pool
            .get()
            .map_err(|error| DomainError::Database(error.to_string()))?;
        client
            .execute(
                "DELETE FROM job_locks WHERE job_type = $1 AND locked_by = $2",
                &[&job_type, &instance_id],
            )
            .map_err(|error| DomainError::Database(format!("{error:?}")))?;
        Ok(())
    }

    fn heartbeat(
        &self,
        id: Uuid,
        job_type: &str,
        instance_id: Uuid,
        at: DateTime<Utc>,
        lock_until: DateTime<Utc>,
    ) -> Result<JobStatus, DomainError> {
        let mut client = self
            .pool
            .get()
            .map_err(|error| DomainError::Database(error.to_string()))?;
        // Owner-only write: only while RUNNING and owned by `instance_id`.
        let refreshed = client
            .execute(
                "UPDATE jobs SET heartbeat_at = $3 \
                 WHERE id = $1 AND instance_id = $2 AND status = 'RUNNING'",
                &[&id, &instance_id, &at],
            )
            .map_err(|error| DomainError::Database(format!("{error:?}")))?;
        // Extend the lease only when this call actually refreshed the job.
        if refreshed == 1 {
            client
                .execute(
                    "UPDATE job_locks SET lock_until = $3 \
                     WHERE job_type = $2 AND locked_by = $1",
                    &[&instance_id, &job_type, &lock_until],
                )
                .map_err(|error| DomainError::Database(format!("{error:?}")))?;
        }
        // Return the current status so the worker can detect a cancellation.
        let row = client
            .query_opt("SELECT status FROM jobs WHERE id = $1", &[&id])
            .map_err(|error| DomainError::Database(error.to_string()))?;
        match row {
            Some(row) => {
                let status: String = row.get("status");
                JobStatus::from_str(&status)
            }
            None => Err(DomainError::NotFound(id)),
        }
    }

    fn set_finished(&self, id: Uuid, finished_at: DateTime<Utc>) -> Result<(), DomainError> {
        let mut client = self
            .pool
            .get()
            .map_err(|error| DomainError::Database(error.to_string()))?;
        let mut tx = client
            .transaction()
            .map_err(|error| DomainError::Database(error.to_string()))?;
        let updated = tx
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
        // The job is now terminal: free the type's lock it owned in the same
        // transaction so a FINISHED job can never leave an orphaned lock (the
        // worker's later explicit `release` becomes a harmless no-op).
        tx.execute(
            "DELETE FROM job_locks jl USING jobs j \
             WHERE j.id = $1 AND jl.job_type = j.job_type \
               AND (j.instance_id IS NULL OR jl.locked_by = j.instance_id)",
            &[&id],
        )
        .map_err(|error| DomainError::Database(error.to_string()))?;
        tx.commit()
            .map_err(|error| DomainError::Database(error.to_string()))?;
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
        let mut tx = client
            .transaction()
            .map_err(|error| DomainError::Database(error.to_string()))?;
        let updated = tx
            .execute(
                "UPDATE jobs SET status = 'FAILED', finished_at = $2, failure_message = $3 \
                 WHERE id = $1 AND status = 'RUNNING'",
                &[&id, &finished_at, &message],
            )
            .map_err(|error| DomainError::Database(error.to_string()))?;
        if updated == 0 {
            return Err(DomainError::InvalidQuery(format!(
                "job {id} can only fail from RUNNING state"
            )));
        }
        // Same as `set_finished`: terminal transitions free the job's own lock.
        tx.execute(
            "DELETE FROM job_locks jl USING jobs j \
             WHERE j.id = $1 AND jl.job_type = j.job_type \
               AND (j.instance_id IS NULL OR jl.locked_by = j.instance_id)",
            &[&id],
        )
        .map_err(|error| DomainError::Database(error.to_string()))?;
        tx.commit()
            .map_err(|error| DomainError::Database(error.to_string()))?;
        Ok(())
    }

    fn request_cancellation(&self, id: Uuid) -> Result<(), DomainError> {
        let mut client = self
            .pool
            .get()
            .map_err(|error| DomainError::Database(error.to_string()))?;
        let updated = client
            .execute(
                "UPDATE jobs SET status = 'CANCELLATION_REQUESTED' \
                 WHERE id = $1 AND status = 'RUNNING'",
                &[&id],
            )
            .map_err(|error| DomainError::Database(error.to_string()))?;
        if updated == 0 {
            return Err(DomainError::InvalidQuery(format!(
                "job {id} is not in RUNNING state and cannot be cancelled"
            )));
        }
        Ok(())
    }

    fn mark_cancelled(&self, id: Uuid, finished_at: DateTime<Utc>) -> Result<(), DomainError> {
        let mut client = self
            .pool
            .get()
            .map_err(|error| DomainError::Database(error.to_string()))?;
        let mut tx = client
            .transaction()
            .map_err(|error| DomainError::Database(error.to_string()))?;
        let updated = tx
            .execute(
                "UPDATE jobs SET status = 'CANCELLED', finished_at = $2, \
                 failure_message = 'cancelled' \
                 WHERE id = $1 AND status IN ('RUNNING', 'CANCELLATION_REQUESTED')",
                &[&id, &finished_at],
            )
            .map_err(|error| DomainError::Database(error.to_string()))?;
        if updated == 1 {
            // The job is now terminal: free the type's lock it owned so a
            // cancelled job can never keep claiming "running elsewhere" when its
            // worker (the usual release path) is gone. Scoped to this job's own
            // instance so a lock that a newer job of the same type acquired is
            // never removed.
            tx.execute(
                "DELETE FROM job_locks jl USING jobs j \
                 WHERE j.id = $1 AND jl.job_type = j.job_type \
                   AND (j.instance_id IS NULL OR jl.locked_by = j.instance_id)",
                &[&id],
            )
            .map_err(|error| DomainError::Database(error.to_string()))?;
            tx.commit()
                .map_err(|error| DomainError::Database(error.to_string()))?;
            return Ok(());
        }
        // No transition happened. A worker's own finalize can re-observe a job
        // that a force-cancel already set to CANCELLED: treat that as an
        // idempotent success. Anything else is not cancellable.
        let row = tx
            .query_opt("SELECT status FROM jobs WHERE id = $1", &[&id])
            .map_err(|error| DomainError::Database(error.to_string()))?;
        match row {
            Some(row) if row.get::<_, String>("status") == "CANCELLED" => {
                tx.commit()
                    .map_err(|error| DomainError::Database(error.to_string()))?;
                Ok(())
            }
            _ => Err(DomainError::InvalidQuery(format!(
                "job {id} is not in a cancellable state"
            ))),
        }
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

    fn find_active_by_type(&self, job_type: &str) -> Result<Vec<Job>, DomainError> {
        let mut client = self
            .pool
            .get()
            .map_err(|error| DomainError::Database(error.to_string()))?;
        let rows = client
            .query(
                &format!(
                    "SELECT {SELECT_COLUMNS} FROM jobs \
                     WHERE job_type = $1 AND status IN ('RUNNING', 'CANCELLATION_REQUESTED') \
                     ORDER BY created_at DESC"
                ),
                &[&job_type],
            )
            .map_err(|error| DomainError::Database(error.to_string()))?;
        rows.iter().map(Self::map_row).collect()
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
                     WHERE job_type = $1 AND status = 'FINISHED' \
                     ORDER BY finished_at DESC NULLS LAST LIMIT 1"
                ),
                &[&job_type],
            )
            .map_err(|error| DomainError::Database(error.to_string()))?;
        row.map(|row| Self::map_row(&row)).transpose()
    }

    fn reconcile_stale_active(
        &self,
        job_type: &str,
        heartbeat_before: DateTime<Utc>,
        now: DateTime<Utc>,
    ) -> Result<(), DomainError> {
        let mut client = self
            .pool
            .get()
            .map_err(|error| DomainError::Database(error.to_string()))?;
        // Stage 1: stale RUNNING -> CANCELLATION_REQUESTED (give the worker a
        // cooperative chance to stop itself).
        client
            .execute(
                "UPDATE jobs SET status = 'CANCELLATION_REQUESTED' \
                 WHERE job_type = $1 AND status = 'RUNNING' \
                   AND (heartbeat_at IS NULL OR heartbeat_at < $2)",
                &[&job_type, &heartbeat_before],
            )
            .map_err(|error| DomainError::Database(format!("{error:?}")))?;
        // Stage 2: stale CANCELLATION_REQUESTED -> CANCELLED (the worker is gone
        // or too slow; force-finalize). The dead owner will never call `release`,
        // so also free the type lock it held (scoped to its instance so a lock a
        // newer job acquired is never removed).
        client
            .execute(
                "WITH cancelled AS ( \
                    UPDATE jobs SET status = 'CANCELLED', finished_at = $3, \
                     failure_message = 'heartbeat lost' \
                     WHERE job_type = $1 AND status = 'CANCELLATION_REQUESTED' \
                       AND (heartbeat_at IS NULL OR heartbeat_at < $2) \
                     RETURNING job_type, instance_id \
                 ) \
                 DELETE FROM job_locks jl USING cancelled c \
                 WHERE jl.job_type = c.job_type \
                   AND (c.instance_id IS NULL OR jl.locked_by = c.instance_id)",
                &[&job_type, &heartbeat_before, &now],
            )
            .map_err(|error| DomainError::Database(format!("{error:?}")))?;
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use std::str::FromStr;

    use chrono::{Duration, Timelike, Utc};
    use postgres::{Config as PostgresConfig, NoTls};
    use serde_json::json;
    use testcontainers::runners::SyncRunner;
    use testcontainers_modules::postgres::Postgres;
    use uuid::Uuid;

    use super::PostgresJobRepository;
    use crate::adapter::driven::postgres::create_pool;
    use crate::core::domain::configuration::configuration::value_objects::DatabaseConfiguration;
    use crate::core::domain::jobs::job::{Job, JobStatus};
    use crate::core::domain::jobs::repository_port::JobRepository;

    const INSTANCE_A: Uuid = Uuid::from_u128(0x0000_0000_0000_0000_0000_0000_0000_00A1);
    const INSTANCE_B: Uuid = Uuid::from_u128(0x0000_0000_0000_0000_0000_0000_0000_00A2);

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

    /// A RUNNING job owned by `instance`.
    fn running_job(id: Uuid, job_type: &str, instance: Uuid) -> Job {
        Job::running(
            id,
            format!("Job {job_type}"),
            job_type.to_string(),
            instance,
            Utc::now(),
        )
    }

    /// Acquires the type's lock and inserts a RUNNING job under it.
    fn start_job(db: &TestDb, id: Uuid, job_type: &str, instance: Uuid) {
        assert!(
            db.repository
                .acquire(job_type, instance, Utc::now() + Duration::hours(1))
                .unwrap()
        );
        db.repository
            .insert(running_job(id, job_type, instance))
            .unwrap();
    }

    /// Postgres TIMESTAMPTZ stores microseconds; `chrono` keeps nanoseconds.
    /// Truncate so round-trip equality checks pass.
    fn micros(dt: chrono::DateTime<Utc>) -> chrono::DateTime<Utc> {
        dt - Duration::nanoseconds((dt.nanosecond() % 1000) as i64)
    }

    #[test]
    fn inserts_and_reads_owned_running_job_round_trip() {
        let db = TestDb::new();
        let id = Uuid::new_v4();
        let started = Utc::now();
        db.repository
            .insert(Job::running(
                id,
                "Data source update".to_string(),
                "data_source_update".to_string(),
                INSTANCE_A,
                started,
            ))
            .unwrap();

        let stored = db.repository.find_by_id(id).unwrap().unwrap();
        assert_eq!(stored.name, "Data source update");
        assert_eq!(stored.status, JobStatus::Running);
        assert_eq!(stored.instance_id, Some(INSTANCE_A));
        assert_eq!(stored.heartbeat_at, Some(micros(started)));
        assert!(stored.metadata.is_empty());
    }

    #[test]
    fn acquire_is_exclusive_until_released() {
        let db = TestDb::new();
        let job_type = "acquire_test";

        assert!(
            db.repository
                .acquire(job_type, INSTANCE_A, Utc::now() + Duration::hours(1))
                .unwrap()
        );
        // A second instance cannot acquire while the lock is held.
        assert!(
            !db.repository
                .acquire(job_type, INSTANCE_B, Utc::now() + Duration::hours(1))
                .unwrap()
        );

        // Release lets another instance acquire.
        db.repository.release(job_type, INSTANCE_A).unwrap();
        assert!(
            db.repository
                .acquire(job_type, INSTANCE_B, Utc::now() + Duration::hours(1))
                .unwrap()
        );
    }

    #[test]
    fn acquire_takes_over_an_expired_lock() {
        let db = TestDb::new();
        let job_type = "expired_lock_test";

        assert!(
            db.repository
                .acquire(job_type, INSTANCE_A, Utc::now() + Duration::hours(1))
                .unwrap()
        );
        // Force the lock to expire, then a different instance can take it over.
        let mut client = db.raw_client();
        client
            .execute(
                "UPDATE job_locks SET lock_until = now() - interval '1 second' WHERE job_type = $1",
                &[&job_type],
            )
            .unwrap();
        drop(client);

        assert!(
            db.repository
                .acquire(job_type, INSTANCE_B, Utc::now() + Duration::hours(1))
                .unwrap()
        );
    }

    #[test]
    fn heartbeat_refreshes_job_and_extends_lease() {
        let db = TestDb::new();
        let job_type = "heartbeat_test";
        let id = Uuid::new_v4();
        start_job(&db, id, job_type, INSTANCE_A);

        let beat = Utc::now();
        let lease = beat + Duration::hours(2);
        let status = db
            .repository
            .heartbeat(id, job_type, INSTANCE_A, beat, lease)
            .unwrap();
        assert_eq!(status, JobStatus::Running);

        let stored = db.repository.find_by_id(id).unwrap().unwrap();
        assert_eq!(stored.heartbeat_at, Some(micros(beat)));

        let mut client = db.raw_client();
        let lock_until: chrono::DateTime<Utc> = client
            .query_one(
                "SELECT lock_until FROM job_locks WHERE job_type = $1",
                &[&job_type],
            )
            .unwrap()
            .get(0);
        drop(client);
        assert_eq!(lock_until, micros(lease), "heartbeat must extend the lease");
    }

    #[test]
    fn heartbeat_from_a_foreign_instance_is_a_noop() {
        let db = TestDb::new();
        let job_type = "foreign_heartbeat_test";
        let id = Uuid::new_v4();
        start_job(&db, id, job_type, INSTANCE_A);
        let original = db.repository.find_by_id(id).unwrap().unwrap();

        // INSTANCE_B does not own the job: the write is a no-op.
        let status = db
            .repository
            .heartbeat(
                id,
                job_type,
                INSTANCE_B,
                Utc::now(),
                Utc::now() + Duration::hours(1),
            )
            .unwrap();
        assert_eq!(status, JobStatus::Running);

        let stored = db.repository.find_by_id(id).unwrap().unwrap();
        assert_eq!(
            stored.heartbeat_at, original.heartbeat_at,
            "a foreign instance must not refresh the heartbeat"
        );
    }

    #[test]
    fn heartbeat_reports_a_cancellation_request() {
        let db = TestDb::new();
        let job_type = "heartbeat_cancel_test";
        let id = Uuid::new_v4();
        start_job(&db, id, job_type, INSTANCE_A);

        db.repository.request_cancellation(id).unwrap();
        let status = db
            .repository
            .heartbeat(
                id,
                job_type,
                INSTANCE_A,
                Utc::now(),
                Utc::now() + Duration::hours(1),
            )
            .unwrap();
        assert_eq!(status, JobStatus::CancellationRequested);
    }

    #[test]
    fn runs_lifecycle_running_to_finished_and_failed() {
        let db = TestDb::new();

        let finished_id = Uuid::new_v4();
        start_job(&db, finished_id, "lifecycle_finished", INSTANCE_A);
        let finished = Utc::now();
        db.repository.set_finished(finished_id, finished).unwrap();
        let done = db.repository.find_by_id(finished_id).unwrap().unwrap();
        assert_eq!(done.status, JobStatus::Finished);
        assert_eq!(done.finished_at, Some(micros(finished)));
        assert!(
            db.repository
                .acquire(
                    "lifecycle_finished",
                    INSTANCE_B,
                    Utc::now() + Duration::hours(1)
                )
                .unwrap(),
            "FINISHED must free the type's lock"
        );

        let failed_id = Uuid::new_v4();
        start_job(&db, failed_id, "lifecycle_failed", INSTANCE_A);
        let failed = Utc::now();
        db.repository
            .set_failed(failed_id, failed, "provider error")
            .unwrap();
        let failed_job = db.repository.find_by_id(failed_id).unwrap().unwrap();
        assert_eq!(failed_job.status, JobStatus::Failed);
        assert_eq!(
            failed_job.failure_message.as_deref(),
            Some("provider error")
        );
        assert!(
            db.repository
                .acquire(
                    "lifecycle_failed",
                    INSTANCE_B,
                    Utc::now() + Duration::hours(1)
                )
                .unwrap(),
            "FAILED must free the type's lock"
        );
    }

    #[test]
    fn request_cancellation_then_mark_cancelled() {
        let db = TestDb::new();
        let job_type = "cancel_flow_test";
        let id = Uuid::new_v4();
        start_job(&db, id, job_type, INSTANCE_A);

        db.repository.request_cancellation(id).unwrap();
        let requested = db.repository.find_by_id(id).unwrap().unwrap();
        assert_eq!(requested.status, JobStatus::CancellationRequested);

        let finished = Utc::now();
        db.repository.mark_cancelled(id, finished).unwrap();
        let cancelled = db.repository.find_by_id(id).unwrap().unwrap();
        assert_eq!(cancelled.status, JobStatus::Cancelled);
        assert_eq!(cancelled.finished_at, Some(micros(finished)));
        assert_eq!(cancelled.failure_message.as_deref(), Some("cancelled"));
        // Cancelling frees the type's lock, so the next run can claim right away
        // even though the (gone) worker never called `release`.
        assert!(
            db.repository
                .acquire(job_type, INSTANCE_B, Utc::now() + Duration::hours(1))
                .unwrap(),
            "a CANCELLED job must release its job_locks row"
        );
    }

    #[test]
    fn force_cancel_marks_running_job_cancelled_directly() {
        let db = TestDb::new();
        let job_type = "force_cancel_test";
        let id = Uuid::new_v4();
        start_job(&db, id, job_type, INSTANCE_A);

        db.repository.mark_cancelled(id, Utc::now()).unwrap();
        let cancelled = db.repository.find_by_id(id).unwrap().unwrap();
        assert_eq!(cancelled.status, JobStatus::Cancelled);
        assert!(
            db.repository
                .acquire(job_type, INSTANCE_B, Utc::now() + Duration::hours(1))
                .unwrap(),
            "a force-cancelled job must release its job_locks row"
        );
    }

    #[test]
    fn mark_cancelled_is_idempotent_when_already_cancelled() {
        let db = TestDb::new();
        let job_type = "cancel_idempotent_test";
        let id = Uuid::new_v4();
        start_job(&db, id, job_type, INSTANCE_A);
        db.repository.mark_cancelled(id, Utc::now()).unwrap();
        // A worker's own finalize can re-observe a job a force-cancel already
        // set to CANCELLED: that must be a no-op success, not an error.
        db.repository.mark_cancelled(id, Utc::now()).unwrap();
        assert_eq!(
            db.repository.find_by_id(id).unwrap().unwrap().status,
            JobStatus::Cancelled
        );
    }

    #[test]
    fn mark_cancelled_rejects_terminal_states() {
        let db = TestDb::new();
        let job_type = "mark_cancel_guard_test";
        let id = Uuid::new_v4();
        start_job(&db, id, job_type, INSTANCE_A);
        db.repository.set_finished(id, Utc::now()).unwrap();

        assert!(db.repository.mark_cancelled(id, Utc::now()).is_err());
    }

    #[test]
    fn find_active_by_type_returns_running_and_requested_jobs() {
        let db = TestDb::new();
        let job_type = "active_test";
        let running_id = Uuid::new_v4();
        db.repository
            .insert(running_job(running_id, job_type, INSTANCE_A))
            .unwrap();
        let requested_id = Uuid::new_v4();
        db.repository
            .insert(running_job(requested_id, job_type, INSTANCE_A))
            .unwrap();
        db.repository.request_cancellation(requested_id).unwrap();

        // A FINISHED job of the same type is not "active".
        let done_id = Uuid::new_v4();
        db.repository
            .insert(running_job(done_id, job_type, INSTANCE_A))
            .unwrap();
        db.repository.set_finished(done_id, Utc::now()).unwrap();

        let active = db.repository.find_active_by_type(job_type).unwrap();
        let mut ids: Vec<Uuid> = active.iter().map(|job| job.id).collect();
        ids.sort();
        let mut expected = vec![requested_id, running_id];
        expected.sort();
        assert_eq!(ids, expected);
    }

    #[test]
    fn reconcile_flips_stale_running_to_cancellation_requested_then_cancelled() {
        let db = TestDb::new();
        let job_type = "reconcile_test";
        let stale_id = Uuid::new_v4();
        start_job(&db, stale_id, job_type, INSTANCE_A);
        // Make the heartbeat old.
        let mut client = db.raw_client();
        client
            .execute(
                "UPDATE jobs SET heartbeat_at = now() - interval '1 hour' WHERE id = $1",
                &[&stale_id],
            )
            .unwrap();
        drop(client);

        let now = Utc::now();
        let heartbeat_before = now - Duration::minutes(30);
        db.repository
            .reconcile_stale_active(job_type, heartbeat_before, now)
            .unwrap();
        let after_first = db.repository.find_by_id(stale_id).unwrap().unwrap();
        assert_eq!(
            after_first.status,
            JobStatus::Cancelled,
            "a dead worker's job is force-cancelled in one reconcile pass"
        );
        // Force-cancelling on behalf of the dead worker also frees its lock, so
        // the type is claimable again immediately.
        assert!(
            db.repository
                .acquire(job_type, INSTANCE_B, Utc::now() + Duration::hours(1))
                .unwrap(),
            "the watcher's force-cancel must release the dead worker's lock"
        );
    }

    #[test]
    fn reconcile_spares_fresh_and_non_running_jobs() {
        let db = TestDb::new();
        let job_type = "reconcile_fresh_test";
        // A freshly heartbeated RUNNING job must survive reconciliation.
        let fresh_id = Uuid::new_v4();
        start_job(&db, fresh_id, job_type, INSTANCE_A);
        db.repository
            .heartbeat(
                fresh_id,
                job_type,
                INSTANCE_A,
                Utc::now(),
                Utc::now() + Duration::hours(1),
            )
            .unwrap();

        // A stale FINISHED job is terminal and must not be touched either.
        let done_id = Uuid::new_v4();
        db.repository
            .insert(running_job(done_id, job_type, INSTANCE_A))
            .unwrap();
        let mut client = db.raw_client();
        client
            .execute(
                "UPDATE jobs SET heartbeat_at = now() - interval '1 hour' \
                 WHERE id = $1 AND status = 'RUNNING'",
                &[&done_id],
            )
            .unwrap();
        client
            .execute(
                "UPDATE jobs SET status = 'FINISHED', finished_at = now() WHERE id = $1",
                &[&done_id],
            )
            .unwrap();
        drop(client);

        let now = Utc::now();
        db.repository
            .reconcile_stale_active(job_type, now - Duration::minutes(30), now)
            .unwrap();

        assert_eq!(
            db.repository.find_by_id(fresh_id).unwrap().unwrap().status,
            JobStatus::Running
        );
        assert_eq!(
            db.repository.find_by_id(done_id).unwrap().unwrap().status,
            JobStatus::Finished
        );
    }

    #[test]
    fn updates_metadata_in_place() {
        let db = TestDb::new();
        let job_type = "metadata_test";
        let id = Uuid::new_v4();
        db.repository
            .insert(running_job(id, job_type, INSTANCE_A))
            .unwrap();

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
            .insert(running_job(running_id, "data_source_update", INSTANCE_A))
            .unwrap();

        let finished_id = Uuid::new_v4();
        db.repository
            .insert(running_job(finished_id, "data_source_update", INSTANCE_A))
            .unwrap();
        db.repository.set_finished(finished_id, Utc::now()).unwrap();

        let cancelled_id = Uuid::new_v4();
        db.repository
            .insert(running_job(cancelled_id, "data_source_update", INSTANCE_A))
            .unwrap();
        db.repository
            .mark_cancelled(cancelled_id, Utc::now())
            .unwrap();

        let other_id = Uuid::new_v4();
        db.repository
            .insert(running_job(other_id, "asset_cleanup", INSTANCE_A))
            .unwrap();

        let all = db.repository.find_all(None, None).unwrap();
        assert_eq!(all.len(), 4);

        let by_type = db
            .repository
            .find_all(Some("data_source_update"), None)
            .unwrap();
        assert_eq!(by_type.len(), 3);

        let cancelled = db
            .repository
            .find_all(Some("data_source_update"), Some(JobStatus::Cancelled))
            .unwrap();
        assert_eq!(cancelled.len(), 1);
        assert_eq!(cancelled[0].id, cancelled_id);

        assert_eq!(
            db.repository
                .find_last_finished_by_type("data_source_update")
                .unwrap()
                .unwrap()
                .id,
            finished_id
        );
    }
}
