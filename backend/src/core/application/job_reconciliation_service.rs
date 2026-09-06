//! Application service for the periodic job watcher.
//!
//! The watcher reconciles stale active jobs of every scheduled type: a RUNNING
//! job whose `heartbeat_at` is older than its type's max heartbeat interval is
//! moved to CANCELLATION_REQUESTED, and a CANCELLATION_REQUESTED job that is
//! still stale is force-finalized as CANCELLED. This recovers jobs whose owning
//! instance died (or stopped heartbeating) without waiting for an operator.
//!
//! It also finalizes **orphaned per-source import runs**: a `data_source_imports`
//! row is normally transitioned by the worker thread that started it, which may
//! be gone (crash/restart) or stuck in a provider call when its aggregate
//! `data_source_update` job is force-cancelled. Such a run would otherwise stay
//! `RUNNING` and the data-sources UI would show a perpetual "Running".

use std::sync::Arc;

use chrono::{DateTime, Duration, Utc};

use crate::core::application::asset_cleanup_service::ASSET_CLEANUP_JOB_TYPE;
use crate::core::application::data_source_update_service::DATA_SOURCE_UPDATE_JOB_TYPE;
use crate::core::application::tiles_update_service::TILES_UPDATE_JOB_TYPE;
use crate::core::domain::configuration::configuration::Configuration;
use crate::core::domain::data_source::import_run_port::DataImportRunRepository;
use crate::core::domain::jobs::repository_port::JobRepository;

/// Grace window before an unlinked (`job_id IS NULL`) RUNNING import run is
/// treated as orphaned. Job-linked orphans need no grace: a run under a terminal
/// job is orphaned by definition.
const ORPHANED_IMPORT_RUN_GRACE: Duration = Duration::minutes(15);

pub struct JobReconciliationService {
    job_repository: Arc<dyn JobRepository + Send + Sync>,
    import_run_repository: Arc<dyn DataImportRunRepository + Send + Sync>,
    configuration: Arc<Configuration>,
}

impl JobReconciliationService {
    pub fn new(
        job_repository: Arc<dyn JobRepository + Send + Sync>,
        import_run_repository: Arc<dyn DataImportRunRepository + Send + Sync>,
        configuration: Arc<Configuration>,
    ) -> Self {
        Self {
            job_repository,
            import_run_repository,
            configuration,
        }
    }

    /// The (job type, heartbeat interval) pairs the watcher reconciles.
    fn heartbeat_intervals(&self) -> Vec<(&'static str, chrono::Duration)> {
        vec![
            (
                DATA_SOURCE_UPDATE_JOB_TYPE,
                self.configuration
                    .data_source_update_max_heartbeat_interval(),
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
        // Finalize per-source import runs orphaned by a force-cancelled or
        // crashed aggregate job, so the data-sources UI can never keep showing a
        // "Running" badge for a source whose job is already terminal.
        match self
            .import_run_repository
            .finalize_orphaned_running(now - ORPHANED_IMPORT_RUN_GRACE)
        {
            Ok(0) => {}
            Ok(finalized) => {
                println!("Finalized {finalized} orphaned data-source import run(s)");
            }
            Err(error) => eprintln!("Failed to finalize orphaned import runs: {error:?}"),
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

    use super::{JobReconciliationService, ORPHANED_IMPORT_RUN_GRACE};
    use crate::core::application::asset_cleanup_service::ASSET_CLEANUP_JOB_TYPE;
    use crate::core::application::data_source_update_service::DATA_SOURCE_UPDATE_JOB_TYPE;
    use crate::core::application::tiles_update_service::TILES_UPDATE_JOB_TYPE;
    use crate::core::domain::configuration::configuration::Configuration;
    use crate::core::domain::configuration::configuration::value_objects::{
        AssetStorageConfiguration, DatabaseConfiguration, MapsConfiguration,
    };
    use crate::core::domain::data_source::data_source::value_objects::Id as DataSourceId;
    use crate::core::domain::data_source::import_run::DataImportRun;
    use crate::core::domain::data_source::import_run_port::DataImportRunRepository;
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

        fn mark_cancelled(
            &self,
            _id: Uuid,
            _finished_at: DateTime<Utc>,
        ) -> Result<(), DomainError> {
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

    /// In-memory import-run store recording reaper invocations. The watcher
    /// never writes runs itself — it only calls `finalize_orphaned_running` —
    /// so all other trait methods are no-ops.
    struct MemoryImportRunRepository {
        reap_calls: Mutex<Vec<DateTime<Utc>>>,
        reap_result: Mutex<Result<u64, String>>,
    }

    impl Default for MemoryImportRunRepository {
        fn default() -> Self {
            Self {
                reap_calls: Mutex::new(Vec::new()),
                reap_result: Mutex::new(Ok(0)),
            }
        }
    }

    impl MemoryImportRunRepository {
        fn set_reap_result(&self, result: Result<u64, String>) {
            *self.reap_result.lock().unwrap() = result;
        }
    }

    impl DataImportRunRepository for MemoryImportRunRepository {
        fn insert(&self, _run: &DataImportRun) -> Result<(), DomainError> {
            Ok(())
        }

        fn finish(&self, _id: Uuid, _finished_at: DateTime<Utc>) -> Result<(), DomainError> {
            Ok(())
        }

        fn fail(
            &self,
            _id: Uuid,
            _finished_at: DateTime<Utc>,
            _message: &str,
        ) -> Result<(), DomainError> {
            Ok(())
        }

        fn latest_by_data_source(
            &self,
            _data_source_id: DataSourceId,
        ) -> Result<Option<DataImportRun>, DomainError> {
            Ok(None)
        }

        fn finalize_orphaned_running(&self, older_than: DateTime<Utc>) -> Result<u64, DomainError> {
            self.reap_calls.lock().unwrap().push(older_than);
            self.reap_result
                .lock()
                .unwrap()
                .clone()
                .map_err(DomainError::Database)
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
            Arc::new(MemoryImportRunRepository::default()),
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

        // First pass: a stale RUNNING job is advanced to CANCELLATION_REQUESTED.
        service.reconcile_all(Utc::now());
        for id in &stale_ids {
            let job = service.job_repository.find_by_id(*id).unwrap().unwrap();
            assert!(
                job.is_cancellation_requested(),
                "a stale job ({id}) must be requested on the first pass, was {:?}",
                job.status
            );
        }

        // Second pass: a still-stale CANCELLATION_REQUESTED job is force-cancelled.
        service.reconcile_all(Utc::now());
        for id in stale_ids {
            let job = service.job_repository.find_by_id(id).unwrap().unwrap();
            assert!(
                job.is_cancelled(),
                "a stale job ({id}) must be force-cancelled on the second pass, was {:?}",
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

    #[test]
    fn reconcile_all_reaps_orphaned_import_runs() {
        let job_repo = Arc::new(MemoryJobRepository::new(Vec::new()));
        let runs = Arc::new(MemoryImportRunRepository::default());
        let service =
            JobReconciliationService::new(job_repo, runs.clone(), Arc::new(configuration()));

        let now = Utc::now();
        service.reconcile_all(now);

        let calls = runs.reap_calls.lock().unwrap();
        assert_eq!(calls.len(), 1, "the watcher must reap orphaned import runs");
        assert_eq!(
            calls[0],
            now - ORPHANED_IMPORT_RUN_GRACE,
            "the reaper is invoked with now minus the unlinked grace window"
        );
    }

    #[test]
    fn reconcile_all_handles_a_nonempty_reap_and_a_reaper_error() {
        // A non-zero finalized count (the Ok(n > 0) branch) is logged without
        // failing the reconciliation.
        let runs = Arc::new(MemoryImportRunRepository::default());
        runs.set_reap_result(Ok(3));
        let service = JobReconciliationService::new(
            Arc::new(MemoryJobRepository::new(Vec::new())),
            runs.clone(),
            Arc::new(configuration()),
        );
        service.reconcile_all(Utc::now());

        // A reaper error is logged and does not abort the watcher.
        let failing = Arc::new(MemoryImportRunRepository::default());
        failing.set_reap_result(Err("reap failed".to_string()));
        let service = JobReconciliationService::new(
            Arc::new(MemoryJobRepository::new(Vec::new())),
            failing.clone(),
            Arc::new(configuration()),
        );
        service.reconcile_all(Utc::now());

        assert_eq!(runs.reap_calls.lock().unwrap().len(), 1);
        assert_eq!(failing.reap_calls.lock().unwrap().len(), 1);
    }
}
