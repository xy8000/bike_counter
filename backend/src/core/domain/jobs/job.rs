//! Generic ShedLock-style job tracking: the persisted representation of an
//! asynchronous job together with its lifecycle status.
//!
//! A job row is only created after its owning instance has acquired the
//! `job_locks` row, so every job starts as RUNNING and carries the owning
//! `instance_id` plus a `heartbeat_at` timestamp the owner refreshes after each
//! sub-task. There is no transient PENDING state.

use std::str::FromStr;

use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use serde_json::Map;
use uuid::Uuid;

use crate::core::domain::error::DomainError;

/// Lifecycle status of a job.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "SCREAMING_SNAKE_CASE")]
pub enum JobStatus {
    Running,
    Finished,
    Failed,
    /// A cooperative cancellation has been requested (via REST or a stale
    /// heartbeat); the owning worker stops at the next sub-task boundary and
    /// finalizes `Cancelled`.
    CancellationRequested,
    Cancelled,
}

impl JobStatus {
    /// The canonical wire / database representation.
    pub fn as_str(&self) -> &'static str {
        match self {
            JobStatus::Running => "RUNNING",
            JobStatus::Finished => "FINISHED",
            JobStatus::Failed => "FAILED",
            JobStatus::CancellationRequested => "CANCELLATION_REQUESTED",
            JobStatus::Cancelled => "CANCELLED",
        }
    }
}

impl FromStr for JobStatus {
    type Err = DomainError;

    fn from_str(value: &str) -> Result<Self, Self::Err> {
        match value {
            "RUNNING" => Ok(JobStatus::Running),
            "FINISHED" => Ok(JobStatus::Finished),
            "FAILED" => Ok(JobStatus::Failed),
            "CANCELLATION_REQUESTED" => Ok(JobStatus::CancellationRequested),
            "CANCELLED" => Ok(JobStatus::Cancelled),
            _ => Err(DomainError::InvalidQuery(format!(
                "unknown job status '{value}'"
            ))),
        }
    }
}

/// A generic job tracked through its lifecycle.
///
/// Every run creates a **new** job row so full history is kept. While a job is
/// RUNNING it owns the type's `job_locks` row until its terminal transition.
#[derive(Debug, Clone)]
pub struct Job {
    pub id: Uuid,
    pub name: String,
    pub job_type: String,
    pub status: JobStatus,
    pub started_at: Option<DateTime<Utc>>,
    pub finished_at: Option<DateTime<Utc>>,
    pub failure_message: Option<String>,
    /// Generic key/value metadata (rendered as a table in the UI later).
    pub metadata: Map<String, serde_json::Value>,
    /// The instance that owns (runs) this job. `None` only for rows that
    /// predate the ownership model (the watcher cancels those as stale).
    pub instance_id: Option<Uuid>,
    /// Last time the owner reported progress (`None` = stale/unknown; the
    /// watcher treats it as such).
    pub heartbeat_at: Option<DateTime<Utc>>,
}

impl Job {
    /// Creates a new RUNNING job owned by `instance_id`, starting now.
    pub fn running(
        id: Uuid,
        name: String,
        job_type: String,
        instance_id: Uuid,
        started_at: DateTime<Utc>,
    ) -> Self {
        Self {
            id,
            name,
            job_type,
            status: JobStatus::Running,
            started_at: Some(started_at),
            finished_at: None,
            failure_message: None,
            metadata: Map::new(),
            instance_id: Some(instance_id),
            heartbeat_at: Some(started_at),
        }
    }

    pub fn is_running(&self) -> bool {
        matches!(self.status, JobStatus::Running)
    }

    pub fn is_successful(&self) -> bool {
        matches!(self.status, JobStatus::Finished)
    }

    /// Whether this job may still be cancelled by an external call. A RUNNING
    /// job can be (cooperatively) cancelled; an already-requested job can still
    /// be force-cancelled.
    pub fn is_cancellable(&self) -> bool {
        matches!(
            self.status,
            JobStatus::Running | JobStatus::CancellationRequested
        )
    }

    pub fn is_cancellation_requested(&self) -> bool {
        matches!(self.status, JobStatus::CancellationRequested)
    }

    pub fn is_cancelled(&self) -> bool {
        matches!(self.status, JobStatus::Cancelled)
    }

    /// Whether the job reached a terminal state (no further transitions).
    pub fn is_terminal(&self) -> bool {
        matches!(
            self.status,
            JobStatus::Finished | JobStatus::Failed | JobStatus::Cancelled
        )
    }
}

#[cfg(test)]
mod tests {
    use std::str::FromStr;

    use chrono::{Duration, Utc};

    use super::*;

    #[test]
    fn running_job_is_running_and_owned() {
        let instance = Uuid::new_v4();
        let started = Utc::now();
        let job = Job::running(
            Uuid::new_v4(),
            "Data source update".to_string(),
            "data_source_update".to_string(),
            instance,
            started,
        );
        assert_eq!(job.status, JobStatus::Running);
        assert!(job.is_running());
        assert!(job.is_cancellable());
        assert!(!job.is_successful());
        assert!(!job.is_cancellation_requested());
        assert!(!job.is_cancelled());
        assert!(!job.is_terminal());
        assert_eq!(job.instance_id, Some(instance));
        assert_eq!(job.heartbeat_at, Some(started));
        assert_eq!(job.started_at, Some(started));
    }

    #[test]
    fn status_round_trips_through_uppercase_string() {
        for (status, text) in [
            (JobStatus::Running, "RUNNING"),
            (JobStatus::Finished, "FINISHED"),
            (JobStatus::Failed, "FAILED"),
            (JobStatus::CancellationRequested, "CANCELLATION_REQUESTED"),
            (JobStatus::Cancelled, "CANCELLED"),
        ] {
            assert_eq!(status.as_str(), text);
            assert_eq!(JobStatus::from_str(text).unwrap(), status);
        }
    }

    #[test]
    fn unknown_status_is_an_invalid_query_error() {
        assert!(matches!(
            JobStatus::from_str("BOGUS"),
            Err(DomainError::InvalidQuery(_))
        ));
    }

    #[test]
    fn terminal_and_auxiliary_status_helpers() {
        let mut cancelled = Job::running(
            Uuid::new_v4(),
            "x".to_string(),
            "t".to_string(),
            Uuid::new_v4(),
            Utc::now(),
        );
        cancelled.status = JobStatus::Cancelled;
        assert!(cancelled.is_cancelled());
        assert!(cancelled.is_terminal());
        assert!(!cancelled.is_cancellable());

        let mut requested = Job::running(
            Uuid::new_v4(),
            "x".to_string(),
            "t".to_string(),
            Uuid::new_v4(),
            Utc::now(),
        );
        requested.status = JobStatus::CancellationRequested;
        assert!(requested.is_cancellation_requested());
        assert!(!requested.is_terminal());
        // An already-requested job can still be force-cancelled (HATEOAS keeps
        // the cancel link until the job is actually terminal).
        assert!(requested.is_cancellable());

        let mut failed = Job::running(
            Uuid::new_v4(),
            "x".to_string(),
            "t".to_string(),
            Uuid::new_v4(),
            Utc::now(),
        );
        failed.status = JobStatus::Failed;
        failed.finished_at = Some(Utc::now() - Duration::seconds(1));
        assert!(failed.is_terminal());
    }
}
