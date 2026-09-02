//! Model: one per-data-source import run.
//!
//! The aggregate `data_source_update` job updates every configured source in one
//! run; each source additionally records its own [`DataImportRun`] so the UI can
//! show per-source last-import facts (status, duration, failure message). The
//! warning/error counts of a run are derived on read by counting the provider
//! messages recorded since `started_at` (a run is the only writer of its source
//! at any moment).

use std::str::FromStr;

use chrono::{DateTime, Utc};
use uuid::Uuid;

use super::data_source::value_objects::Id as DataSourceId;
use crate::core::domain::error::DomainError;

/// Lifecycle status of a single per-source import run.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ImportRunStatus {
    Running,
    Finished,
    Failed,
}

impl ImportRunStatus {
    /// The canonical wire / database representation.
    pub fn as_str(&self) -> &'static str {
        match self {
            ImportRunStatus::Running => "RUNNING",
            ImportRunStatus::Finished => "FINISHED",
            ImportRunStatus::Failed => "FAILED",
        }
    }
}

impl FromStr for ImportRunStatus {
    type Err = DomainError;

    fn from_str(value: &str) -> Result<Self, Self::Err> {
        match value {
            "RUNNING" => Ok(ImportRunStatus::Running),
            "FINISHED" => Ok(ImportRunStatus::Finished),
            "FAILED" => Ok(ImportRunStatus::Failed),
            _ => Err(DomainError::InvalidQuery(format!(
                "unknown import run status '{value}'"
            ))),
        }
    }
}

/// A single per-data-source import run.
#[derive(Debug, Clone)]
pub struct DataImportRun {
    pub id: Uuid,
    pub data_source_id: DataSourceId,
    /// The aggregate `data_source_update` job this run belongs to (optional so
    /// runs can exist independently of the job bookkeeping).
    pub job_id: Option<Uuid>,
    pub started_at: DateTime<Utc>,
    pub finished_at: Option<DateTime<Utc>>,
    pub status: ImportRunStatus,
    pub failure_message: Option<String>,
}

impl DataImportRun {
    /// A new RUNNING run that started at `now`.
    pub fn start(
        id: Uuid,
        data_source_id: DataSourceId,
        job_id: Option<Uuid>,
        now: DateTime<Utc>,
    ) -> Self {
        Self {
            id,
            data_source_id,
            job_id,
            started_at: now,
            finished_at: None,
            status: ImportRunStatus::Running,
            failure_message: None,
        }
    }

    /// Whether this run reports a failure (and thus drives the "last import
    /// failed" warning in the UI).
    pub fn failed(&self) -> bool {
        matches!(self.status, ImportRunStatus::Failed)
    }

    /// The run duration in seconds when it has finished (`finished_at` set).
    pub fn duration_seconds(&self) -> Option<i64> {
        self.finished_at
            .map(|finished_at| (finished_at - self.started_at).num_seconds())
    }
}

#[cfg(test)]
mod tests {
    use std::str::FromStr;

    use chrono::{Duration, Utc};

    use super::*;

    #[test]
    fn status_round_trips_through_uppercase_strings() {
        for (status, raw) in [
            (ImportRunStatus::Running, "RUNNING"),
            (ImportRunStatus::Finished, "FINISHED"),
            (ImportRunStatus::Failed, "FAILED"),
        ] {
            assert_eq!(status.as_str(), raw);
            assert_eq!(ImportRunStatus::from_str(raw).unwrap(), status);
        }
    }

    #[test]
    fn unknown_status_is_an_invalid_query_error() {
        assert!(matches!(
            ImportRunStatus::from_str("BOGUS"),
            Err(DomainError::InvalidQuery(_))
        ));
    }

    #[test]
    fn start_creates_a_running_run_without_finish() {
        let now = Utc::now();
        let run = DataImportRun::start(Uuid::new_v4(), DataSourceId(Uuid::new_v4()), None, now);
        assert_eq!(run.status, ImportRunStatus::Running);
        assert_eq!(run.finished_at, None);
        assert!(!run.failed());
        assert_eq!(run.duration_seconds(), None);
    }

    #[test]
    fn finished_run_reports_duration() {
        let now = Utc::now();
        let mut run = DataImportRun::start(Uuid::new_v4(), DataSourceId(Uuid::new_v4()), None, now);
        run.finished_at = Some(now + Duration::seconds(42));
        run.status = ImportRunStatus::Finished;
        assert!(!run.failed());
        assert_eq!(run.duration_seconds(), Some(42));
    }

    #[test]
    fn failed_run_is_detected() {
        let now = Utc::now();
        let mut run = DataImportRun::start(Uuid::new_v4(), DataSourceId(Uuid::new_v4()), None, now);
        run.status = ImportRunStatus::Failed;
        run.failure_message = Some("boom".to_string());
        assert!(run.failed());
    }
}
