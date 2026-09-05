//! Application service for the periodic job watcher.
//!
//! The watcher reconciles stale active jobs of every scheduled type: a RUNNING
//! job whose `heartbeat_at` is older than its type's max heartbeat interval is
//! moved to CANCELLATION_REQUESTED, and a CANCELLATION_REQUESTED job that is
//! still stale is force-finalized as CANCELLED. This recovers jobs whose owning
//! instance died (or stopped heartbeating) without waiting for an operator.

use std::sync::Arc;

use chrono::{DateTime, Utc};

use crate::core::application::asset_cleanup_service::ASSET_CLEANUP_JOB_TYPE;
use crate::core::application::data_source_update_service::DATA_SOURCE_UPDATE_JOB_TYPE;
use crate::core::application::tiles_update_service::TILES_UPDATE_JOB_TYPE;
use crate::core::domain::configuration::configuration::Configuration;
use crate::core::domain::jobs::repository_port::JobRepository;

pub struct JobReconciliationService {
    job_repository: Arc<dyn JobRepository + Send + Sync>,
    configuration: Arc<Configuration>,
}

impl JobReconciliationService {
    pub fn new(
        job_repository: Arc<dyn JobRepository + Send + Sync>,
        configuration: Arc<Configuration>,
    ) -> Self {
        Self {
            job_repository,
            configuration,
        }
    }

    /// The (job type, heartbeat interval) pairs the watcher reconciles.
    fn heartbeat_intervals(&self) -> Vec<(&'static str, chrono::Duration)> {
        vec![
            (
                DATA_SOURCE_UPDATE_JOB_TYPE,
                self.configuration.data_source_update_max_heartbeat_interval(),
            ),
            (
                ASSET_CLEANUP_JOB_TYPE,
                self.configuration.asset_cleanup_max_heartbeat_interval(),
            ),
            (
                TILES_UPDATE_JOB_TYPE,
                self.configuration.maps().update_max_heartbeat_interval(),
            ),
        ]
    }

    /// Reconciles every scheduled job type against `now`: any active job whose
    /// heartbeat is older than its type's interval is advanced one step towards
    /// CANCELLED. Failures are logged and do not stop the other types.
    pub fn reconcile_all(&self, now: DateTime<Utc>) {
        for (job_type, interval) in self.heartbeat_intervals() {
            let heartbeat_before = now - interval;
            match self
                .job_repository
                .reconcile_stale_active(job_type, heartbeat_before, now)
            {
                Ok(()) => {}
                Err(error) => eprintln!("Failed to reconcile stale {job_type} jobs: {error:?}"),
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use std::collections::HashMap;
    use std::sync::{Arc, Mutex};

    use chrono::{DateTime, Duration, Utc};
    use serde_json::Value;
    use uuid::Uuid;

    use super::JobReconciliationService;
    use crate::core::application::asset_cleanup_service::ASSET_CLEANUP_JOB_TYPE;
    use crate::core::application::data_source_update_service::DATA_SOURCE_UPDATE_JOB_TYPE;
    use crate::core::application::tiles_update_service::TILES_UPDATE_JOB_TYPE;
    use crate::core::domain::configuration::configuration::value_objects::{
        AssetStorageConfiguration, DatabaseConfiguration, MapsConfiguration,
    };
    use crate::core::domain::configuration::configuration::Configuration;
    use crate::core::domain::error::DomainError;
    use crate::core::domain::jobs::job::{Job, JobStatus};
    use crate::core::domain::jobs::repository_port::JobRepository;

    const INSTANCE: Uuid = Uuid::from_u128(0x0000_0000_0000_0000_0000_0000_0000_00F2);

    /// Minimal in-memory repository: only insert/find_all/reconcile are used.
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
            _job_type: &str,
            _instance_id: Uuid,
            _lock_until: DateTime<Utc>,
        ) -> Result<bool, DomainError> {
            Ok(true)
        }

        fn release(&self, _job_type: &str, _instance_id: Uuid) -> Result<(), DomainError> {
            Ok(())
        }

        fn heartbeat(
            &self,
            id: Uuid,
            _job_type: &str,
            _instance_id: Uuid,
            at: DateTime<Utc>,
            _lock_until: DateTime<Utc>,
        ) -> Result<JobStatus, DomainError> {
            let mut jobs = self.jobs.lock().unwrap();
            let job = jobs
                .iter_mut()
                .find(|job| job.id == id)
                .ok_or(DomainError::NotFound(id))?;
            job.heartbeat_at = Some(at);
            Ok(job.status)
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

        fn request_cancellation(&self, _id: Uuid) -> Result<(), DomainError> {
            Ok(())
        }

        fn mark_cancelled(&self, _id: Uuid, _finished_at: DateTime<Utc>) -> Result<(), DomainError> {
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
            _job_type: Option<&str>,
            _status: Option<JobStatus>,
        ) -> Result<Vec<Job>, DomainError> {
            Ok(self.jobs.lock().unwrap().clone())
        }

        fn find_active_by_type(&self, job_type: &str) -> Result<Vec<Job>, DomainError> {
            Ok(self
                .jobs
                .lock()
                .unwrap()
                .iter()
                .filter(|job| job.job_type == job_type)
                .cloned()
                .collect())
        }

        fn find_last_finished_by_type(&self, _job_type: &str) -> Result<Option<Job>, DomainError> {
            Ok(None)
        }

        fn reconcile_stale_active(
            &self,
            job_type: &str,
            heartbeat_before: DateTime<Utc>,
            now: DateTime<Utc>,
        ) -> Result<(), DomainError> {
            let mut jobs = self.jobs.lock().unwrap();
            for job in jobs.iter_mut() {
                if job.job_type != job_type {
                    continue;
                }
                let stale = job.heartbeat_at.is_none_or(|beat| beat < heartbeat_before);
                match job.status {
                    JobStatus::Running if stale => job.status = JobStatus::CancellationRequested,
                    JobStatus::CancellationRequested if stale => {
                        job.status = JobStatus::Cancelled;
                        job.finished_at = Some(now);
                    }
                    _ => {}
                }
            }
            Ok(())
        }
    }

    fn configuration() -> Configuration {
        let database = DatabaseConfiguration::new(
            "url".to_string(),
            "user".to_string(),
            "password".to_string(),
            "database".to_string(),
        )
        .unwrap();
        let asset_storage = AssetStorageConfiguration::new(
            "http://minio:9000".to_string(),
            "key".to_string(),
            "secret".to_string(),
            "bucket".to_string(),
            "region".to_string(),
        )
        .unwrap();
        let maps = MapsConfiguration::new(
            "0 0 3 1 1,3,5,7,9,11 *".to_string(),
            3600,
            "https://example.com/source.pmtiles".to_string(),
            "1.0.0".to_string(),
        )
        .unwrap();
        Configuration::new(
            database,
            vec![],
            "0 0 * * * *".to_string(),
            600,
            asset_storage,
            "0 0 4 * * *".to_string(),
            600,
            maps,
        )
        .unwrap()
    }

    /// A RUNNING job of `job_type` heartbeated `minutes_ago` minutes ago.
    fn stale_running(job_type: &'static str, minutes_ago: i64) -> Job {
        let started = Utc::now() - Duration::minutes(minutes_ago);
        let mut job = Job::running(
            Uuid::new_v4(),
            format!("Job {job_type}"),
            job_type.to_string(),
            INSTANCE,
            started,
        );
        job.heartbeat_at = Some(started);
        job
    }

    fn service(jobs: Vec<Job>) -> JobReconciliationService {
        JobReconciliationService::new(
            Arc::new(MemoryJobRepository::new(jobs)),
            Arc::new(configuration()),
        )
    }

    #[test]
    fn reconcile_all_cancels_stale_jobs_of_every_type() {
        // One stale RUNNING job per type (older than their 600 s interval) plus a
        // fresh one that must survive.
        let stale_data = stale_running(DATA_SOURCE_UPDATE_JOB_TYPE, 3600);
        let stale_cleanup = stale_running(ASSET_CLEANUP_JOB_TYPE, 3600);
        let stale_tiles = stale_running(TILES_UPDATE_JOB_TYPE, 3600);
        let fresh = stale_running(DATA_SOURCE_UPDATE_JOB_TYPE, 0);
        let jobs = vec![stale_data, stale_cleanup, stale_tiles, fresh];
        // Keep a clone of the ids before moving.
        let stale_ids: Vec<Uuid> = jobs.iter().take(3).map(|job| job.id).collect();
        let service = service(jobs);

        service.reconcile_all(Utc::now());

        for id in stale_ids {
            let job = service
                .job_repository
                .find_by_id(id)
                .unwrap()
                .unwrap();
            assert!(
                job.is_cancelled(),
                "a stale job ({id}) must be force-cancelled, was {:?}",
                job.status
            );
        }
        // The fresh job is still RUNNING.
        let fresh_jobs = service
            .job_repository
            .find_active_by_type(DATA_SOURCE_UPDATE_JOB_TYPE)
            .unwrap();
        assert_eq!(fresh_jobs.len(), 1);
        assert!(fresh_jobs[0].is_running());
    }
}
