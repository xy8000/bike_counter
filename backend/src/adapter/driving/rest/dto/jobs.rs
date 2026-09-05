use std::collections::HashMap;

use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use utoipa::{IntoParams, ToSchema};
use uuid::Uuid;

use super::LinkDto;
use crate::core::domain::jobs::job::{Job, JobStatus};

/// Lifecycle status of a job, as exposed by the API.
#[derive(Debug, Clone, Copy, Serialize, Deserialize, ToSchema, PartialEq, Eq)]
#[serde(rename_all = "UPPERCASE")]
pub enum JobStatusDto {
    Pending,
    Running,
    Finished,
    Failed,
}

impl From<JobStatus> for JobStatusDto {
    fn from(status: JobStatus) -> Self {
        match status {
            JobStatus::Pending => JobStatusDto::Pending,
            JobStatus::Running => JobStatusDto::Running,
            JobStatus::Finished => JobStatusDto::Finished,
            JobStatus::Failed => JobStatusDto::Failed,
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
    /// Absolute deadline until which a RUNNING job blocks other runs.
    pub lifetime_until: DateTime<Utc>,
    pub max_lifetime_exceeded: bool,
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

        Self {
            id,
            name: job.name,
            job_type: job.job_type,
            status: job.status.into(),
            started_at: job.started_at,
            finished_at: job.finished_at,
            failure_message: job.failure_message,
            metadata: serde_json::Value::Object(job.metadata),
            lifetime_until: job.lifetime_until,
            max_lifetime_exceeded: job.max_lifetime_exceeded,
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

#[derive(Debug, Deserialize, ToSchema, IntoParams)]
pub struct JobQueryParams {
    pub job_type: Option<String>,
    pub status: Option<String>,
}
