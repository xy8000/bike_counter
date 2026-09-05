use chrono::{DateTime, Utc};
use serde_json::Value;
use uuid::Uuid;

use super::job::{Job, JobStatus};
use crate::core::domain::error::DomainError;

/// Repository for ShedLock-style generic jobs.
///
/// Mutual exclusion is delegated to a dedicated `job_locks` table: the only
/// racy step is [`Self::acquire`] (one atomic `INSERT ... ON CONFLICT` upsert);
/// once a job is RUNNING, all status transitions are single conditional
/// `UPDATE`s guarded by the current status and the owning `instance_id`.
pub trait JobRepository {
    /// Persists a new (already RUNNING, owned) job. Callers must have acquired
    /// the type's lock first via [`Self::acquire`].
    fn insert(&self, job: Job) -> Result<(), DomainError>;

    /// Atomically claims the job type for `instance_id` until `lock_until` on
    /// the `job_locks` table (ShedLock-style upsert). Returns `false` when
    /// another instance still holds an unexpired lock; the caller should skip.
    fn acquire(
        &self,
        job_type: &str,
        instance_id: Uuid,
        lock_until: DateTime<Utc>,
    ) -> Result<bool, DomainError>;

    /// Releases the type's lock. Only meaningful when `instance_id` is still
    /// the lock owner (e.g. at the terminal transition).
    fn release(&self, job_type: &str, instance_id: Uuid) -> Result<(), DomainError>;

    /// Owner-only liveness report: refreshes the job's `heartbeat_at` and
    /// extends the type's `job_locks` lease to `lock_until` (the owner passes
    /// `at + heartbeat_interval`). Updates only apply while the row is RUNNING
    /// and owned by `instance_id` (a foreign caller's write is a no-op).
    /// Returns the job's current status so the worker can detect a
    /// `CANCELLATION_REQUESTED`/`CANCELLED` transition; `DomainError::NotFound`
    /// if the job id is unknown.
    fn heartbeat(
        &self,
        id: Uuid,
        job_type: &str,
        instance_id: Uuid,
        at: DateTime<Utc>,
        lock_until: DateTime<Utc>,
    ) -> Result<JobStatus, DomainError>;

    /// Marks a RUNNING job FINISHED and records its finish time.
    fn set_finished(&self, id: Uuid, finished_at: DateTime<Utc>) -> Result<(), DomainError>;

    /// Marks a RUNNING job FAILED and records its finish time + message.
    fn set_failed(
        &self,
        id: Uuid,
        finished_at: DateTime<Utc>,
        message: &str,
    ) -> Result<(), DomainError>;

    /// Requests cooperative cancellation: RUNNING -> CANCELLATION_REQUESTED.
    fn request_cancellation(&self, id: Uuid) -> Result<(), DomainError>;

    /// Force-finalizes cancellation: RUNNING or CANCELLATION_REQUESTED ->
    /// CANCELLED, recording the finish time and a `cancelled` message.
    fn mark_cancelled(&self, id: Uuid, finished_at: DateTime<Utc>) -> Result<(), DomainError>;

    /// Updates a single key in the job's metadata map.
    fn update_metadata(&self, id: Uuid, key: &str, value: Value) -> Result<(), DomainError>;

    fn find_by_id(&self, id: Uuid) -> Result<Option<Job>, DomainError>;

    /// Lists jobs, optionally filtered by job type and/or status.
    fn find_all(
        &self,
        job_type: Option<&str>,
        status: Option<JobStatus>,
    ) -> Result<Vec<Job>, DomainError>;

    /// The active (RUNNING or CANCELLATION_REQUESTED) jobs of the given type —
    /// used to skip starting a new run while one is in flight or being
    /// finalized.
    fn find_active_by_type(&self, job_type: &str) -> Result<Vec<Job>, DomainError>;

    /// The most recently FINISHED job of the given type, if any (used to decide
    /// whether a job has ever succeeded).
    fn find_last_finished_by_type(&self, job_type: &str) -> Result<Option<Job>, DomainError>;

    /// Watcher reconciliation: atomically flips RUNNING jobs of the type whose
    /// `heartbeat_at` is older than `heartbeat_before` to CANCELLATION_REQUESTED,
    /// and flips CANCELLATION_REQUESTED jobs past the threshold to CANCELLED.
    /// Rows with a `NULL` heartbeat are treated as stale.
    fn reconcile_stale_active(
        &self,
        job_type: &str,
        heartbeat_before: DateTime<Utc>,
        now: DateTime<Utc>,
    ) -> Result<(), DomainError>;
}
