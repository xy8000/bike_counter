//! Application service exposing job reads and cancellation through the core.

use std::sync::Arc;

use chrono::Utc;
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

    /// Cancels a job (see [`JobServicePort::cancel`]).
    pub fn cancel(&self, id: Uuid, force: bool) -> Result<Job, DomainError> {
        let job = self
            .repository
            .find_by_id(id)?
            .ok_or(DomainError::NotFound(id))?;
        match job.status {
            JobStatus::Finished | JobStatus::Failed | JobStatus::Cancelled => {
                return Err(DomainError::InvalidQuery(format!(
                    "job {id} is already in a terminal state and cannot be cancelled"
                )));
            }
            // force=false on an already-requested job is idempotent.
            JobStatus::CancellationRequested => {
                if force {
                    self.repository.mark_cancelled(id, Utc::now())?;
                }
            }
            JobStatus::Running => {
                if force {
                    self.repository.mark_cancelled(id, Utc::now())?;
                } else {
                    self.repository.request_cancellation(id)?;
                }
            }
        }
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

    fn cancel(&self, id: Uuid, force: bool) -> Result<Job, DomainError> {
        self.cancel(id, force)
    }
}

#[cfg(test)]
mod tests {
    use std::collections::HashMap;
    use std::sync::{Arc, Mutex};

    use chrono::{DateTime, Duration, Utc};
    use serde_json::Value;
    use uuid::Uuid;

    use super::JobService;
    use crate::core::domain::error::DomainError;
    use crate::core::domain::jobs::job::{Job, JobStatus};
    use crate::core::domain::jobs::repository_port::JobRepository;
    use crate::core::domain::jobs::service_port::JobServicePort;

    const INSTANCE: Uuid = Uuid::from_u128(0x0000_0000_0000_0000_0000_0000_0000_00F1);

    /// In-memory job + lock store implementing the repository port by mutating
    /// its state, mirroring the Postgres repository closely enough for service
    /// tests.
    struct MemoryJobRepository {
        jobs: Mutex<Vec<Job>>,
        locks: Mutex<HashMap<String, (Uuid, DateTime<Utc>)>>,
    }

    impl MemoryJobRepository {
        fn new(jobs: Vec<Job>) -> Self {
            Self {
                jobs: Mutex::new(jobs),
                locks: Mutex::new(HashMap::new()),
            }
        }
    }

    impl JobRepository for MemoryJobRepository {
        fn insert(&self, job: Job) -> Result<(), DomainError> {
            self.jobs.lock().unwrap().push(job);
            Ok(())
        }

        fn acquire(
            &self,
            job_type: &str,
            instance_id: Uuid,
            lock_until: DateTime<Utc>,
        ) -> Result<bool, DomainError> {
            let mut locks = self.locks.lock().unwrap();
            match locks.get(job_type) {
                Some((_, until)) if *until >= Utc::now() => Ok(false),
                _ => {
                    locks.insert(job_type.to_string(), (instance_id, lock_until));
                    Ok(true)
                }
            }
        }

        fn release(&self, job_type: &str, instance_id: Uuid) -> Result<(), DomainError> {
            let mut locks = self.locks.lock().unwrap();
            if locks
                .get(job_type)
                .is_some_and(|(locked_by, _)| *locked_by == instance_id)
            {
                locks.remove(job_type);
            }
            Ok(())
        }

        fn heartbeat(
            &self,
            id: Uuid,
            _job_type: &str,
            instance_id: Uuid,
            at: DateTime<Utc>,
            _lock_until: DateTime<Utc>,
        ) -> Result<JobStatus, DomainError> {
            let mut jobs = self.jobs.lock().unwrap();
            if let Some(job) = jobs.iter_mut().find(|job| job.id == id) {
                if job.instance_id == Some(instance_id) && job.status == JobStatus::Running {
                    job.heartbeat_at = Some(at);
                }
                Ok(job.status)
            } else {
                Err(DomainError::NotFound(id))
            }
        }

        fn set_finished(&self, id: Uuid, finished_at: DateTime<Utc>) -> Result<(), DomainError> {
            let mut jobs = self.jobs.lock().unwrap();
            let job = jobs
                .iter_mut()
                .find(|job| job.id == id)
                .ok_or(DomainError::NotFound(id))?;
            if job.status != JobStatus::Running {
                return Err(DomainError::InvalidQuery(
                    "not running".to_string(),
                ));
            }
            job.status = JobStatus::Finished;
            job.finished_at = Some(finished_at);
            Ok(())
        }

        fn set_failed(
            &self,
            id: Uuid,
            finished_at: DateTime<Utc>,
            message: &str,
        ) -> Result<(), DomainError> {
            let mut jobs = self.jobs.lock().unwrap();
            let job = jobs
                .iter_mut()
                .find(|job| job.id == id)
                .ok_or(DomainError::NotFound(id))?;
            if job.status != JobStatus::Running {
                return Err(DomainError::InvalidQuery("not running".to_string()));
            }
            job.status = JobStatus::Failed;
            job.finished_at = Some(finished_at);
            job.failure_message = Some(message.to_string());
            Ok(())
        }

        fn request_cancellation(&self, id: Uuid) -> Result<(), DomainError> {
            let mut jobs = self.jobs.lock().unwrap();
            let job = jobs
                .iter_mut()
                .find(|job| job.id == id)
                .ok_or(DomainError::NotFound(id))?;
            if job.status != JobStatus::Running {
                return Err(DomainError::InvalidQuery("not running".to_string()));
            }
            job.status = JobStatus::CancellationRequested;
            Ok(())
        }

        fn mark_cancelled(&self, id: Uuid, finished_at: DateTime<Utc>) -> Result<(), DomainError> {
            let mut jobs = self.jobs.lock().unwrap();
            let job = jobs
                .iter_mut()
                .find(|job| job.id == id)
                .ok_or(DomainError::NotFound(id))?;
            if !matches!(
                job.status,
                JobStatus::Running | JobStatus::CancellationRequested
            ) {
                return Err(DomainError::InvalidQuery(
                    "not cancellable".to_string(),
                ));
            }
            job.status = JobStatus::Cancelled;
            job.finished_at = Some(finished_at);
            job.failure_message = Some("cancelled".to_string());
            Ok(())
        }

        fn update_metadata(&self, _id: Uuid, _key: &str, _value: Value) -> Result<(), DomainError> {
            Ok(())
        }

        fn find_by_id(&self, id: Uuid) -> Result<Option<Job>, DomainError> {
            Ok(self
                .jobs
                .lock()
                .unwrap()
                .iter()
                .find(|job| job.id == id)
                .cloned())
        }

        fn find_all(
            &self,
            job_type: Option<&str>,
            status: Option<JobStatus>,
        ) -> Result<Vec<Job>, DomainError> {
            let mut jobs = self.jobs.lock().unwrap().clone();
            if let Some(job_type) = job_type {
                jobs.retain(|job| job.job_type == job_type);
            }
            if let Some(status) = status {
                jobs.retain(|job| job.status == status);
            }
            Ok(jobs)
        }

        fn find_active_by_type(&self, job_type: &str) -> Result<Vec<Job>, DomainError> {
            Ok(self
                .jobs
                .lock()
                .unwrap()
                .iter()
                .filter(|job| {
                    job.job_type == job_type
                        && matches!(
                            job.status,
                            JobStatus::Running | JobStatus::CancellationRequested
                        )
                })
                .cloned()
                .collect())
        }

        fn find_last_finished_by_type(&self, job_type: &str) -> Result<Option<Job>, DomainError> {
            Ok(self
                .jobs
                .lock()
                .unwrap()
                .iter()
                .filter(|job| job.job_type == job_type && job.status == JobStatus::Finished)
                .max_by_key(|job| job.finished_at)
                .cloned())
        }

        fn reconcile_stale_active(
            &self,
            job_type: &str,
            heartbeat_before: DateTime<Utc>,
            _now: DateTime<Utc>,
        ) -> Result<(), DomainError> {
            let mut jobs = self.jobs.lock().unwrap();
            for job in jobs.iter_mut() {
                if job.job_type != job_type {
                    continue;
                }
                let stale = job
                    .heartbeat_at
                    .is_none_or(|beat| beat < heartbeat_before);
                match job.status {
                    JobStatus::Running if stale => {
                        job.status = JobStatus::CancellationRequested
                    }
                    JobStatus::CancellationRequested if stale => {
                        job.status = JobStatus::Cancelled;
                        job.failure_message = Some("heartbeat lost".to_string());
                    }
                    _ => {}
                }
            }
            Ok(())
        }
    }

    /// A RUNNING job of the data-source-update type.
    fn running(id: Uuid) -> Job {
        Job::running(
            id,
            "Data source update".to_string(),
            "data_source_update".to_string(),
            INSTANCE,
            Utc::now(),
        )
    }

    fn service() -> JobService {
        let mut finished = running(Uuid::from_u128(0x31));
        finished.status = JobStatus::Finished;
        finished.finished_at = Some(Utc::now() - Duration::minutes(5));

        let running_job = running(Uuid::from_u128(0x32));

        let mut failed = running(Uuid::from_u128(0x33));
        failed.job_type = "report_generation".to_string();
        failed.status = JobStatus::Failed;
        failed.finished_at = Some(Utc::now() - Duration::minutes(1));

        JobService::new(Arc::new(MemoryJobRepository::new(vec![
            finished,
            running_job,
            failed,
        ])))
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
    fn cancel_running_job_requests_cancellation_without_force() {
        let service = service();
        let id = Uuid::from_u128(0x32);
        let updated = service.cancel(id, false).unwrap();
        assert_eq!(updated.status, JobStatus::CancellationRequested);
        // The in-memory repository actually mutated the stored job.
        assert_eq!(
            service.find_by_id(id).unwrap().status,
            JobStatus::CancellationRequested
        );
    }

    #[test]
    fn cancel_running_job_with_force_marks_cancelled() {
        let service = service();
        let id = Uuid::from_u128(0x32);
        let updated = service.cancel(id, true).unwrap();
        assert_eq!(updated.status, JobStatus::Cancelled);
        assert!(updated.is_terminal());
    }

    #[test]
    fn cancel_already_requested_job_is_idempotent_without_force_and_cancelled_with_force() {
        let mut requested = running(Uuid::from_u128(0x34));
        requested.status = JobStatus::CancellationRequested;
        let service = JobService::new(Arc::new(MemoryJobRepository::new(vec![requested])));

        let id = Uuid::from_u128(0x34);
        assert_eq!(
            service.cancel(id, false).unwrap().status,
            JobStatus::CancellationRequested
        );
        assert_eq!(
            service.cancel(id, true).unwrap().status,
            JobStatus::Cancelled
        );
    }

    #[test]
    fn cancel_terminal_job_is_invalid() {
        let service = service();
        assert!(matches!(
            service.cancel(Uuid::from_u128(0x31), false),
            Err(DomainError::InvalidQuery(_))
        ));
        assert!(matches!(
            service.cancel(Uuid::from_u128(0x31), true),
            Err(DomainError::InvalidQuery(_))
        ));
    }

    #[test]
    fn cancel_unknown_id_is_not_found() {
        assert!(matches!(
            service().cancel(Uuid::from_u128(0x99), false),
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
        assert_eq!(
            port.cancel(Uuid::from_u128(0x32), false)
                .unwrap()
                .status,
            JobStatus::CancellationRequested
        );
    }
}
