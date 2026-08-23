//! Application service exposing job reads through the core.

use std::sync::Arc;

use uuid::Uuid;

use crate::core::domain::error::DomainError;
use crate::core::domain::jobs::job::{Job, JobStatus};
use crate::core::domain::jobs::repository_port::JobRepository;
use crate::core::domain::jobs::service_port::JobServicePort;

pub struct JobService {
    repository: Arc<dyn JobRepository + Send + Sync>,
}

impl JobService {
    pub fn new(repository: Arc<dyn JobRepository + Send + Sync>) -> Self {
        Self { repository }
    }

    /// Lists jobs, optionally filtered by job type and/or status.
    pub fn list(
        &self,
        job_type: Option<String>,
        status: Option<JobStatus>,
    ) -> Result<Vec<Job>, DomainError> {
        self.repository.find_all(job_type.as_deref(), status)
    }

    /// Returns a single job; `DomainError::NotFound` if unknown.
    pub fn find_by_id(&self, id: Uuid) -> Result<Job, DomainError> {
        self.repository
            .find_by_id(id)?
            .ok_or(DomainError::NotFound(id))
    }
}

impl JobServicePort for JobService {
    fn list(
        &self,
        job_type: Option<String>,
        status: Option<JobStatus>,
    ) -> Result<Vec<Job>, DomainError> {
        self.list(job_type, status)
    }

    fn find_by_id(&self, id: Uuid) -> Result<Job, DomainError> {
        self.find_by_id(id)
    }
}

#[cfg(test)]
mod tests {
    use std::sync::Arc;

    use chrono::{DateTime, Duration, Utc};
    use serde_json::Value;
    use uuid::Uuid;

    use super::JobService;
    use crate::core::domain::error::DomainError;
    use crate::core::domain::jobs::job::{Job, JobStatus};
    use crate::core::domain::jobs::repository_port::JobRepository;
    use crate::core::domain::jobs::service_port::JobServicePort;

    struct MemoryJobRepository {
        jobs: Vec<Job>,
    }

    impl JobRepository for MemoryJobRepository {
        fn insert(&self, _job: Job) -> Result<(), DomainError> {
            Ok(())
        }

        fn set_running(&self, _id: Uuid, _started_at: DateTime<Utc>) -> Result<(), DomainError> {
            Ok(())
        }

        fn set_finished(&self, _id: Uuid, _finished_at: DateTime<Utc>) -> Result<(), DomainError> {
            Ok(())
        }

        fn set_failed(
            &self,
            _id: Uuid,
            _finished_at: DateTime<Utc>,
            _message: &str,
        ) -> Result<(), DomainError> {
            Ok(())
        }

        fn update_metadata(&self, _id: Uuid, _key: &str, _value: Value) -> Result<(), DomainError> {
            Ok(())
        }

        fn find_by_id(&self, id: Uuid) -> Result<Option<Job>, DomainError> {
            Ok(self.jobs.iter().find(|job| job.id == id).cloned())
        }

        fn find_all(
            &self,
            job_type: Option<&str>,
            status: Option<JobStatus>,
        ) -> Result<Vec<Job>, DomainError> {
            let mut jobs = self.jobs.clone();
            if let Some(job_type) = job_type {
                jobs.retain(|job| job.job_type == job_type);
            }
            if let Some(status) = status {
                jobs.retain(|job| job.status == status);
            }
            Ok(jobs)
        }

        fn find_running_by_type(&self, _job_type: &str) -> Result<Option<Job>, DomainError> {
            Ok(None)
        }

        fn find_last_finished_by_type(&self, _job_type: &str) -> Result<Option<Job>, DomainError> {
            Ok(None)
        }

        fn expire_running_jobs(
            &self,
            _job_type: &str,
            _now: DateTime<Utc>,
        ) -> Result<u64, DomainError> {
            Ok(0)
        }
    }

    fn job(id: Uuid, job_type: &str, status: JobStatus) -> Job {
        let mut job = Job::new(
            id,
            "Job".to_string(),
            job_type.to_string(),
            Utc::now() + Duration::hours(1),
        );
        job.status = status;
        job
    }

    fn service() -> JobService {
        JobService::new(Arc::new(MemoryJobRepository {
            jobs: vec![
                job(
                    Uuid::from_u128(0x31),
                    "data_source_update",
                    JobStatus::Finished,
                ),
                job(
                    Uuid::from_u128(0x32),
                    "data_source_update",
                    JobStatus::Running,
                ),
                job(
                    Uuid::from_u128(0x33),
                    "report_generation",
                    JobStatus::Failed,
                ),
            ],
        }))
    }

    #[test]
    fn list_without_filters_returns_all_jobs() {
        let jobs = service().list(None, None).unwrap();
        assert_eq!(jobs.len(), 3);
    }

    #[test]
    fn list_filters_by_job_type_and_status() {
        let service = service();
        let by_type = service
            .list(Some("data_source_update".to_string()), None)
            .unwrap();
        assert_eq!(by_type.len(), 2);

        let running = service.list(None, Some(JobStatus::Running)).unwrap();
        assert_eq!(running.len(), 1);
        assert_eq!(running[0].id, Uuid::from_u128(0x32));

        let both = service
            .list(
                Some("report_generation".to_string()),
                Some(JobStatus::Failed),
            )
            .unwrap();
        assert_eq!(both.len(), 1);
    }

    #[test]
    fn find_by_id_returns_the_job() {
        let job = service().find_by_id(Uuid::from_u128(0x31)).unwrap();
        assert_eq!(job.job_type, "data_source_update");
    }

    #[test]
    fn find_by_unknown_id_is_not_found() {
        assert!(matches!(
            service().find_by_id(Uuid::from_u128(0x99)),
            Err(DomainError::NotFound(_))
        ));
    }

    #[test]
    fn port_trait_delegates_to_the_service() {
        let service = service();
        let port: &dyn JobServicePort = &service;
        assert_eq!(port.list(None, None).unwrap().len(), 3);
        assert_eq!(
            port.find_by_id(Uuid::from_u128(0x31)).unwrap().job_type,
            "data_source_update"
        );
    }
}
