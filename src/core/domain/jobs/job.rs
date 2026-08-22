//! Generic ShedLock-style job tracking: the persisted representation of an
//! asynchronous job together with its lifecycle status.

use std::str::FromStr;

use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use serde_json::Map;
use uuid::Uuid;

use crate::core::domain::error::DomainError;

/// Lifecycle status of a job.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "UPPERCASE")]
pub enum JobStatus {
    Pending,
    Running,
    Finished,
    Failed,
}

impl JobStatus {
    /// The canonical wire / database representation.
    pub fn as_str(&self) -> &'static str {
        match self {
            JobStatus::Pending => "PENDING",
            JobStatus::Running => "RUNNING",
            JobStatus::Finished => "FINISHED",
            JobStatus::Failed => "FAILED",
        }
    }
}

impl FromStr for JobStatus {
    type Err = DomainError;

    fn from_str(value: &str) -> Result<Self, Self::Err> {
        match value {
            "PENDING" => Ok(JobStatus::Pending),
            "RUNNING" => Ok(JobStatus::Running),
            "FINISHED" => Ok(JobStatus::Finished),
            "FAILED" => Ok(JobStatus::Failed),
            _ => Err(DomainError::InvalidQuery(format!(
                "unknown job status '{value}'"
            ))),
        }
    }
}

/// A generic job tracked through its lifecycle.
///
/// Every run creates a **new** job row so full history is kept. While a job is
/// RUNNING it blocks other runs of the same type only until its `lifetime_until`
/// deadline; afterwards the scheduler expires it (see `expire_running_jobs`).
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
    /// Absolute deadline (TIMESTAMPTZ) until which a RUNNING job may block
    /// other runs (ShedLock `lockAtMostFor`). The job only "lives" before this
    /// timestamp; afterwards it is expired. Must be in the future; there is no
    /// default anywhere.
    pub lifetime_until: DateTime<Utc>,
    pub max_lifetime_exceeded: bool,
}

impl Job {
    /// Creates a new PENDING job. `lifetime_until` is an absolute deadline; the
    /// repository refuses to persist a job whose deadline is not in the future.
    pub fn new(id: Uuid, name: String, job_type: String, lifetime_until: DateTime<Utc>) -> Self {
        Self {
            id,
            name,
            job_type,
            status: JobStatus::Pending,
            started_at: None,
            finished_at: None,
            failure_message: None,
            metadata: Map::new(),
            lifetime_until,
            max_lifetime_exceeded: false,
        }
    }

    pub fn is_running(&self) -> bool {
        matches!(self.status, JobStatus::Running)
    }

    pub fn is_successful(&self) -> bool {
        matches!(self.status, JobStatus::Finished)
    }

    /// Whether this job's `lifetime_until` deadline has been passed by `now`
    /// (the job only "lives" before its deadline timestamp).
    pub fn lifetime_exceeded(&self, now: DateTime<Utc>) -> bool {
        now > self.lifetime_until
    }
}

#[cfg(test)]
mod tests {
    use std::str::FromStr;

    use chrono::{Duration, Utc};

    use super::*;

    #[test]
    fn new_job_is_pending_without_lifetime_exceeded() {
        let job = Job::new(
            Uuid::new_v4(),
            "Data source update".to_string(),
            "data_source_update".to_string(),
            Utc::now() + Duration::seconds(3600),
        );
        assert_eq!(job.status, JobStatus::Pending);
        assert!(!job.is_successful());
        assert!(!job.is_running());
        assert!(!job.max_lifetime_exceeded);
    }

    #[test]
    fn status_round_trips_through_uppercase_string() {
        for (status, text) in [
            (JobStatus::Pending, "PENDING"),
            (JobStatus::Running, "RUNNING"),
            (JobStatus::Finished, "FINISHED"),
            (JobStatus::Failed, "FAILED"),
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
    fn lifetime_exceeded_depends_on_deadline() {
        let now = Utc::now();
        let expired = Job::new(
            Uuid::new_v4(),
            "x".to_string(),
            "t".to_string(),
            now - Duration::seconds(61),
        );
        assert!(expired.lifetime_exceeded(now));

        let alive = Job::new(
            Uuid::new_v4(),
            "x".to_string(),
            "t".to_string(),
            now + Duration::seconds(61),
        );
        assert!(!alive.lifetime_exceeded(now));
    }
}
