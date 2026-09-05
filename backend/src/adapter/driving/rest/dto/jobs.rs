use std::collections::HashMap;

use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use utoipa::{IntoParams, ToSchema};
use uuid::Uuid;

use super::LinkDto;
use crate::core::domain::jobs::job::{Job, JobStatus};

/// Lifecycle status of a job, as exposed by the API.
#[derive(Debug, Clone, Copy, Serialize, Deserialize, ToSchema, PartialEq, Eq)]
#[serde(rename_all = "SCREAMING_SNAKE_CASE")]
pub enum JobStatusDto {
    Running,
    Finished,
    Failed,
    CancellationRequested,
    Cancelled,
}

impl From<JobStatus> for JobStatusDto {
    fn from(status: JobStatus) -> Self {
        match status {
            JobStatus::Running => JobStatusDto::Running,
            JobStatus::Finished => JobStatusDto::Finished,
            JobStatus::Failed => JobStatusDto::Failed,
            JobStatus::CancellationRequested => JobStatusDto::CancellationRequested,
            JobStatus::Cancelled => JobStatusDto::Cancelled,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, ToSchema)]
pub struct JobDto {
    pub id: Uuid,
    pub name: String,
    pub job_type: String,
    pub status: JobStatusDto,
    pub started_at: Option<DateTime<Utc>>,
    pub finished_at: Option<DateTime<Utc>>,
    pub failure_message: Option<String>,
    /// Generic key/value metadata. The `data_source_update` job groups every
    /// source's progress under its data-source UUID, e.g.
    /// `<data-source-uuid>_processed_measurements`,
    /// `<data-source-uuid>_added_measurements` and `<data-source-uuid>_status`.
    #[schema(value_type = Object)]
    pub metadata: serde_json::Value,
    /// The instance that owns (runs) this job; `null` only for rows created
    /// before the ownership model.
    pub instance_id: Option<Uuid>,
    /// Last time the owning instance reported progress.
    pub heartbeat_at: Option<DateTime<Utc>>,
    #[serde(rename = "_links")]
    pub links: HashMap<String, LinkDto>,
}

impl From<Job> for JobDto {
    fn from(job: Job) -> Self {
        let id = job.id;
        let mut links = HashMap::new();
        links.insert(
            "self".to_string(),
            LinkDto::new(format!("/api/v1/jobs/{id}")),
        );
        links.insert("collection".to_string(), LinkDto::new("/api/v1/jobs"));
        links.insert("root".to_string(), LinkDto::new("/api/v1"));
        // A RUNNING job can still be cancelled; expose the action as a link.
        if job.is_cancellable() {
            links.insert(
                "cancel".to_string(),
                LinkDto::new(format!("/api/v1/jobs/{id}/cancel")),
            );
        }

        Self {
            id,
            name: job.name,
            job_type: job.job_type,
            status: job.status.into(),
            started_at: job.started_at,
            finished_at: job.finished_at,
            failure_message: job.failure_message,
            metadata: serde_json::Value::Object(job.metadata),
            instance_id: job.instance_id,
            heartbeat_at: job.heartbeat_at,
            links,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, ToSchema)]
pub struct JobListDto {
    pub items: Vec<JobDto>,
    #[serde(rename = "_links")]
    pub links: HashMap<String, LinkDto>,
}

impl JobListDto {
    pub fn new(jobs: Vec<Job>) -> Self {
        let items = jobs.into_iter().map(JobDto::from).collect();
        let mut links = HashMap::new();
        links.insert("self".to_string(), LinkDto::new("/api/v1/jobs"));
        links.insert("root".to_string(), LinkDto::new("/api/v1"));

        Self { items, links }
    }
}

/// Body of the cancel endpoint. `force = true` marks the job CANCELLED
/// immediately; the default (`false`) requests a cooperative cancellation
/// (CANCELLATION_REQUESTED) that the owning worker finalizes.
#[derive(Debug, Clone, Default, Serialize, Deserialize, ToSchema)]
pub struct CancelJobRequestDto {
    /// Whether to force-cancel immediately instead of requesting a cooperative
    /// stop. Defaults to `false`.
    #[serde(default)]
    pub force: bool,
}

#[derive(Debug, Deserialize, ToSchema, IntoParams)]
pub struct JobQueryParams {
    pub job_type: Option<String>,
    pub status: Option<String>,
}
