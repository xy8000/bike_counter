use chrono::{DateTime, Utc};
use serde_json::Value;
use uuid::Uuid;

use super::job::{Job, JobStatus};
use crate::core::domain::error::DomainError;

/// Repository for ShedLock-style generic jobs.
pub trait JobRepository {
    /// Inserts a new job. Requires a `lifetime_until` deadline in the future;
    /// fails with a domain error otherwise (there is no default anywhere).
    fn insert(&self, job: Job) -> Result<(), DomainError>;

    /// Marks a PENDING job as RUNNING and records its start time.
    fn set_running(&self, id: Uuid, started_at: DateTime<Utc>) -> Result<(), DomainError>;

    /// Marks a RUNNING job as FINISHED and records its finish time.
    fn set_finished(&self, id: Uuid, finished_at: DateTime<Utc>) -> Result<(), DomainError>;

    /// Marks a job as FAILED. Valid from both RUNNING and PENDING.
    fn set_failed(
        &self,
        id: Uuid,
        finished_at: DateTime<Utc>,
        message: &str,
    ) -> Result<(), DomainError>;

    /// Updates a single key in the job's metadata map.
    fn update_metadata(&self, id: Uuid, key: &str, value: Value) -> Result<(), DomainError>;

    fn find_by_id(&self, id: Uuid) -> Result<Option<Job>, DomainError>;

    /// Lists jobs, optionally filtered by job type and/or status.
    fn find_all(
        &self,
        job_type: Option<&str>,
        status: Option<JobStatus>,
    ) -> Result<Vec<Job>, DomainError>;

    /// The RUNNING job of the given type, if any.
    fn find_running_by_type(&self, job_type: &str) -> Result<Option<Job>, DomainError>;

    /// The most recently FINISHED job of the given type, if any (used to decide
    /// whether a job has ever succeeded).
    fn find_last_finished_by_type(&self, job_type: &str) -> Result<Option<Job>, DomainError>;

    /// Atomically flips RUNNING jobs of the given type whose
    /// `started_at + max_lifetime < now` to FAILED, setting
    /// `max_lifetime_exceeded = true` and a failure message. Returns the number
    /// of jobs that were expired.
    fn expire_running_jobs(&self, job_type: &str, now: DateTime<Utc>) -> Result<u64, DomainError>;
}
