//! Driving (inbound) port for job reads. Implemented by `JobService`; consumed
//! by the REST jobs handlers.

use uuid::Uuid;

use crate::core::domain::error::DomainError;
use crate::core::domain::jobs::job::{Job, JobStatus};

pub trait JobServicePort: Send + Sync {
    /// Lists jobs, optionally filtered by job type and/or status.
    fn list(
        &self,
        job_type: Option<String>,
        status: Option<JobStatus>,
    ) -> Result<Vec<Job>, DomainError>;

    /// Returns a single job; `DomainError::NotFound` if unknown.
    fn find_by_id(&self, id: Uuid) -> Result<Job, DomainError>;
}
