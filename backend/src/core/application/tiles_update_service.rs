//! Application job runner that refreshes the self-hosted vector basemap
//! (`tiles/map.pmtiles`) on the configured `[maps]` cron schedule.
//!
//! Mirrors [`AssetCleanupService`](super::asset_cleanup_service::AssetCleanupService)
//! and [`DataSourceUpdateService`](super::data_source_update_service::DataSourceUpdateService):
//! it is driven by the generic cron scheduler through
//! [`ScheduledJobPort`](crate::core::domain::jobs::scheduled_job_port::ScheduledJobPort)
//! and tracks itself as a `tiles_update` job. The actual build is delegated to
//! the [`TilesProvisioningPort`](crate::core::domain::tiles::provisioning_port::TilesProvisioningPort)
//! (the `TilesInit` adapter), which swaps the archive in atomically so the
//! running application stays online during the update.
//!
//! Multi-instance cancellation: the service first claims the type's `job_locks`
//! row (only the winner proceeds), then records a RUNNING job owned by this
//! instance. The atomic provisioning build cannot be interrupted, so the job is
//! heartbeated and checked for a cancellation request immediately before and
//! after the build; a request observed after the build records the job as
//! CANCELLED instead of FINISHED.

use std::str::FromStr;
use std::sync::Arc;

use chrono::{DateTime, Utc};
use uuid::Uuid;

use super::job_heartbeat::JobHeartbeat;
use crate::core::domain::configuration::configuration::Configuration;
use crate::core::domain::error::DomainError;
use crate::core::domain::jobs::job::{Job, JobStatus};
use crate::core::domain::jobs::repository_port::JobRepository;
use crate::core::domain::jobs::scheduled_job_port::ScheduledJobPort;
use crate::core::domain::tiles::provisioning_port::TilesProvisioningPort;

/// The job type owned by this service.
pub const TILES_UPDATE_JOB_TYPE: &str = "tiles_update";
/// Human-readable name of the tiles update job.
pub const TILES_UPDATE_JOB_NAME: &str = "tiles update";

pub struct TilesUpdateService {
    job_repository: Arc<dyn JobRepository + Send + Sync>,
    tiles_provisioning: Arc<dyn TilesProvisioningPort>,
    configuration: Arc<Configuration>,
    instance_id: Uuid,
}

impl TilesUpdateService {
    pub fn new(
        job_repository: Arc<dyn JobRepository + Send + Sync>,
        tiles_provisioning: Arc<dyn TilesProvisioningPort>,
        configuration: Arc<Configuration>,
        instance_id: Uuid,
    ) -> Self {
        Self {
            job_repository,
            tiles_provisioning,
            configuration,
            instance_id,
        }
    }

    /// Decides whether the tiles update job should run now and executes it if
    /// so. Same always-on rule as the data-source update and asset cleanup jobs:
    /// run at startup (never succeeded) and whenever the last successful run is
    /// overdue; skip while an active (RUNNING or awaiting-finalize) job exists.
    pub fn run_if_due(&self) {
        let now = Utc::now();

        match self
            .job_repository
            .find_active_by_type(TILES_UPDATE_JOB_TYPE)
        {
            Ok(active) if !active.is_empty() => {
                let count = active.len();
                let ids = active
                    .iter()
                    .map(|job| job.id.to_string())
                    .collect::<Vec<_>>()
                    .join(", ");
                println!(
                    "Tiles update job is still active ({count} running/requesting: {ids}); \
                     skipping"
                );
                return;
            }
            Ok(_) => {}
            Err(error) => {
                eprintln!("Failed to check for an active tiles update job: {error:?}");
                return;
            }
        }

        match self
            .job_repository
            .find_last_finished_by_type(TILES_UPDATE_JOB_TYPE)
        {
            Ok(None) => {
                println!("Tiles update job has never succeeded; running");
                self.execute(now);
            }
            Ok(Some(last)) => {
                if self.is_overdue(&last, now) {
                    println!(
                        "Tiles update job is overdue (last run {} at {}); running",
                        last.id,
                        last.finished_at
                            .map(|ts| ts.to_rfc3339())
                            .unwrap_or_else(|| "unknown".to_string())
                    );
                    self.execute(now);
                }
            }
            Err(error) => {
                eprintln!("Failed to check the last finished tiles update job: {error:?}");
            }
        }
    }

    /// Whether the last successful run is overdue: the next scheduled cron
    /// trigger after its finish time has already passed.
    fn is_overdue(&self, last: &Job, now: DateTime<Utc>) -> bool {
        let schedule = match cron::Schedule::from_str(self.configuration.maps().update_cron()) {
            Ok(schedule) => schedule,
            // The configuration validates the cron expression at construction.
            Err(_) => return false,
        };
        match last.finished_at.or(last.started_at) {
            Some(anchor) => schedule
                .after(&anchor)
                .next()
                .is_some_and(|next| next <= now),
            None => true,
        }
    }

    /// Runs one tiles update as a tracked job owned by this instance.
    fn execute(&self, now: DateTime<Utc>) {
        let interval = self.configuration.maps().update_max_heartbeat_interval();
        let instance_id = self.instance_id;

        // 1. Claim the type's lock; only the winning instance proceeds.
        match self
            .job_repository
            .acquire(TILES_UPDATE_JOB_TYPE, instance_id, now + interval)
        {
            Ok(true) => {}
            Ok(false) => {
                println!("Tiles update is already active elsewhere (job_locks held); skipping");
                return;
            }
            Err(error) => {
                eprintln!("Failed to acquire the tiles update lock: {error:?}");
                return;
            }
        }

        // 2. Record the RUNNING job owned by this instance.
        let job = Job::running(
            Uuid::new_v4(),
            TILES_UPDATE_JOB_NAME.to_string(),
            TILES_UPDATE_JOB_TYPE.to_string(),
            instance_id,
            now,
        );
        let job_id = job.id;
        let job_name = job.name.clone();
        if let Err(error) = self.job_repository.insert(job) {
            let _ = self
                .job_repository
                .release(TILES_UPDATE_JOB_TYPE, instance_id);
            eprintln!("Failed to record tiles update job {job_name} ({job_id}): {error:?}");
            return;
        }
        println!("Tiles update job {job_name} ({job_id}) started");

        // 3. A dedicated heartbeat loop keeps the job fresh on a fixed tick for
        //    the whole (potentially long, atomic) build, independent of the
        //    boundary checks below.
        let heartbeat = JobHeartbeat::start(
            self.job_repository.clone(),
            job_id,
            TILES_UPDATE_JOB_TYPE,
            instance_id,
            interval,
        );

        // 4. Heartbeat + cancellation check before the atomic build.
        if self.is_cancelled_or_requested(job_id) {
            heartbeat.stop();
            self.finalize_cancelled(job_id, &job_name);
            return;
        }

        // 5. Build (atomic; cannot be interrupted mid-way).
        match self.run_update() {
            Ok(()) => {
                heartbeat.stop();
                self.finalize_after_update(job_id, &job_name);
            }
            Err(error) => {
                heartbeat.stop();
                let message = format!("{error:?}");
                if let Err(set_failed_error) =
                    self.job_repository.set_failed(job_id, Utc::now(), &message)
                {
                    eprintln!(
                        "Failed to mark tiles update job {job_name} ({job_id}) as failed: {set_failed_error:?}"
                    );
                } else {
                    eprintln!("Tiles update job {job_name} ({job_id}) failed: {error:?}");
                }
                let _ = self
                    .job_repository
                    .release(TILES_UPDATE_JOB_TYPE, self.instance_id);
            }
        }
    }

    /// Heartbeats the job and returns whether a cancellation is already in
    /// flight (so the run stops instead of finishing).
    fn is_cancelled_or_requested(&self, job_id: Uuid) -> bool {
        let now = Utc::now();
        let interval = self.configuration.maps().update_max_heartbeat_interval();
        match self.job_repository.heartbeat(
            job_id,
            TILES_UPDATE_JOB_TYPE,
            self.instance_id,
            now,
            now + interval,
        ) {
            Ok(JobStatus::CancellationRequested) | Ok(JobStatus::Cancelled) => true,
            Ok(_) => false,
            Err(error) => {
                eprintln!("Failed to heartbeat tiles update job {job_id}: {error:?}");
                false
            }
        }
    }

    /// Decides FINISHED vs CANCELLED after the (atomic) build completed, then
    /// releases the type's lock.
    fn finalize_after_update(&self, job_id: Uuid, job_name: &str) {
        // A cancellation requested during the build is honored: record CANCELLED
        // (the archive swap is atomic and already happened) instead of FINISHED.
        if self.is_cancelled_or_requested(job_id) {
            self.finalize_cancelled(job_id, job_name);
            return;
        }
        if let Err(error) = self.job_repository.set_finished(job_id, Utc::now()) {
            eprintln!("Failed to finish tiles update job {job_name} ({job_id}): {error:?}");
        } else {
            println!("Tiles update job {job_name} ({job_id}) finished");
        }
        let _ = self
            .job_repository
            .release(TILES_UPDATE_JOB_TYPE, self.instance_id);
    }

    /// Marks the job CANCELLED and releases the type's lock.
    fn finalize_cancelled(&self, job_id: Uuid, job_name: &str) {
        match self.job_repository.mark_cancelled(job_id, Utc::now()) {
            Ok(()) => println!("Tiles update job {job_name} ({job_id}) cancelled"),
            Err(error) => {
                // Already terminal (e.g. force-cancelled elsewhere): fine.
                eprintln!(
                    "Could not finalize tiles update job {job_name} ({job_id}) as cancelled: {error:?}"
                );
            }
        }
        let _ = self
            .job_repository
            .release(TILES_UPDATE_JOB_TYPE, self.instance_id);
    }

    /// Delegates the actual (atomic) rebuild to the provisioning adapter.
    fn run_update(&self) -> Result<(), DomainError> {
        self.tiles_provisioning
            .update()
            .map_err(DomainError::Provider)
    }
}

impl ScheduledJobPort for TilesUpdateService {
    fn run_if_due(&self) {
        self.run_if_due();
    }
}

#[cfg(test)]
mod tests {
    use std::collections::HashMap;
    use std::sync::{Arc, Mutex};

    use chrono::{DateTime, Duration, Utc};
    use serde_json::Value;
    use uuid::Uuid;

    use super::*;
    use crate::core::domain::configuration::configuration::value_objects::{
        AssetStorageConfiguration, DatabaseConfiguration, MapsConfiguration,
    };
    use crate::core::domain::configuration::configuration::{
        Configuration, DEFAULT_ASSET_CLEANUP_CRON, DEFAULT_DATA_SOURCE_UPDATE_CRON,
        DEFAULT_MAPS_UPDATE_CRON,
    };
    use crate::core::domain::tiles::provisioning_port::TilesProvisioningPort;

    const INSTANCE: Uuid = Uuid::from_u128(0x0000_0000_0000_0000_0000_0000_0000_00C1);

    fn configuration() -> Arc<Configuration> {
        Arc::new(
            Configuration::new(
                DatabaseConfiguration::new(
                    "postgres://localhost:5432".to_string(),
                    "user".to_string(),
                    "password".to_string(),
                    "database".to_string(),
                )
                .unwrap(),
                Vec::new(),
                DEFAULT_DATA_SOURCE_UPDATE_CRON.to_string(),
                3600,
                AssetStorageConfiguration::new(
                    "http://minio:9000".to_string(),
                    "minioadmin".to_string(),
                    "minioadmin".to_string(),
                    "bike-counter-images".to_string(),
                    "us-east-1".to_string(),
                )
                .unwrap(),
                DEFAULT_ASSET_CLEANUP_CRON.to_string(),
                3600,
                MapsConfiguration::new(
                    DEFAULT_MAPS_UPDATE_CRON.to_string(),
                    7200,
                    "https://build.protomaps.com/20260905.pmtiles".to_string(),
                    "1.31.2".to_string(),
                )
                .unwrap(),
            )
            .unwrap(),
        )
    }

    struct MockTilesProvisioning {
        updates: Mutex<u32>,
        fail: bool,
    }

    impl MockTilesProvisioning {
        fn new(fail: bool) -> Self {
            Self {
                updates: Mutex::new(0),
                fail,
            }
        }

        fn update_count(&self) -> u32 {
            *self.updates.lock().unwrap()
        }
    }

    impl TilesProvisioningPort for MockTilesProvisioning {
        fn ensure_available(&self) -> Result<(), String> {
            Ok(())
        }

        fn update(&self) -> Result<(), String> {
            *self.updates.lock().unwrap() += 1;
            if self.fail {
                Err("protomaps unreachable".to_string())
            } else {
                Ok(())
            }
        }
    }

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

        fn all(&self) -> Vec<Job> {
            self.jobs.lock().unwrap().clone()
        }

        fn force_status(&self, id: Uuid, status: JobStatus) {
            let mut jobs = self.jobs.lock().unwrap();
            if let Some(job) = jobs.iter_mut().find(|job| job.id == id) {
                job.status = status;
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
                .is_some_and(|(owner, _)| *owner == instance_id)
            {
                locks.remove(job_type);
            }
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
            if let Some(job) = jobs.iter_mut().find(|job| job.id == id) {
                job.heartbeat_at = Some(at);
                Ok(job.status)
            } else {
                Err(DomainError::NotFound(id))
            }
        }

        fn set_finished(&self, id: Uuid, finished_at: DateTime<Utc>) -> Result<(), DomainError> {
            let mut jobs = self.jobs.lock().unwrap();
            if let Some(job) = jobs.iter_mut().find(|job| job.id == id) {
                job.status = JobStatus::Finished;
                job.finished_at = Some(finished_at);
            }
            Ok(())
        }

        fn set_failed(
            &self,
            id: Uuid,
            finished_at: DateTime<Utc>,
            message: &str,
        ) -> Result<(), DomainError> {
            let mut jobs = self.jobs.lock().unwrap();
            if let Some(job) = jobs.iter_mut().find(|job| job.id == id) {
                job.status = JobStatus::Failed;
                job.finished_at = Some(finished_at);
                job.failure_message = Some(message.to_string());
            }
            Ok(())
        }

        fn request_cancellation(&self, id: Uuid) -> Result<(), DomainError> {
            self.force_status(id, JobStatus::CancellationRequested);
            Ok(())
        }

        fn mark_cancelled(&self, id: Uuid, finished_at: DateTime<Utc>) -> Result<(), DomainError> {
            let mut jobs = self.jobs.lock().unwrap();
            if let Some(job) = jobs.iter_mut().find(|job| job.id == id)
                && matches!(
                    job.status,
                    JobStatus::Running | JobStatus::CancellationRequested
                )
            {
                job.status = JobStatus::Cancelled;
                job.finished_at = Some(finished_at);
                job.failure_message = Some("cancelled".to_string());
            }
            Ok(())
        }

        fn update_metadata(&self, id: Uuid, key: &str, value: Value) -> Result<(), DomainError> {
            let mut jobs = self.jobs.lock().unwrap();
            if let Some(job) = jobs.iter_mut().find(|job| job.id == id) {
                job.metadata.insert(key.to_string(), value);
            }
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

        fn find_last_finished_by_type(&self, job_type: &str) -> Result<Option<Job>, DomainError> {
            Ok(self
                .jobs
                .lock()
                .unwrap()
                .iter()
                .filter(|job| job.job_type == job_type && job.status == JobStatus::Finished)
                .cloned()
                .max_by_key(|job| job.finished_at))
        }

        fn reconcile_stale_active(
            &self,
            _job_type: &str,
            _heartbeat_before: DateTime<Utc>,
            _now: DateTime<Utc>,
        ) -> Result<(), DomainError> {
            Ok(())
        }
    }

    fn finished_job(now: DateTime<Utc>) -> Job {
        let mut job = Job::running(
            Uuid::new_v4(),
            TILES_UPDATE_JOB_NAME.to_string(),
            TILES_UPDATE_JOB_TYPE.to_string(),
            INSTANCE,
            now - Duration::minutes(10),
        );
        job.status = JobStatus::Finished;
        job.finished_at = Some(now);
        job
    }

    fn running_job(now: DateTime<Utc>) -> Job {
        Job::running(
            Uuid::new_v4(),
            TILES_UPDATE_JOB_NAME.to_string(),
            TILES_UPDATE_JOB_TYPE.to_string(),
            INSTANCE,
            now,
        )
    }

    fn service(
        repo: Arc<MemoryJobRepository>,
        provisioning: Arc<MockTilesProvisioning>,
    ) -> TilesUpdateService {
        TilesUpdateService::new(repo, provisioning, configuration(), INSTANCE)
    }

    #[test]
    fn runs_when_never_succeeded() {
        let repo = Arc::new(MemoryJobRepository::new(vec![]));
        let provisioning = Arc::new(MockTilesProvisioning::new(false));

        service(repo.clone(), provisioning.clone()).run_if_due();

        assert_eq!(provisioning.update_count(), 1);
    }

    #[test]
    fn skips_while_an_active_job_exists() {
        let now = Utc::now();
        let repo = Arc::new(MemoryJobRepository::new(vec![running_job(now)]));
        let provisioning = Arc::new(MockTilesProvisioning::new(false));

        service(repo.clone(), provisioning.clone()).run_if_due();

        assert_eq!(provisioning.update_count(), 0);
    }

    #[test]
    fn runs_when_overdue_and_records_finished() {
        let now = Utc::now();
        let repo = Arc::new(MemoryJobRepository::new(vec![finished_job(
            now - Duration::days(200),
        )]));
        let provisioning = Arc::new(MockTilesProvisioning::new(false));

        service(repo.clone(), provisioning.clone()).run_if_due();

        assert_eq!(provisioning.update_count(), 1);
        let jobs = repo.all();
        assert!(
            jobs.iter().any(|job| job.status == JobStatus::Finished),
            "expected a FINISHED tiles_update job"
        );
    }

    #[test]
    fn does_not_run_when_the_last_run_is_recent() {
        let now = Utc::now();
        let repo = Arc::new(MemoryJobRepository::new(vec![finished_job(now)]));
        let provisioning = Arc::new(MockTilesProvisioning::new(false));

        service(repo.clone(), provisioning.clone()).run_if_due();

        assert_eq!(provisioning.update_count(), 0);
    }

    #[test]
    fn records_failure_when_the_update_fails() {
        let repo = Arc::new(MemoryJobRepository::new(vec![]));
        let provisioning = Arc::new(MockTilesProvisioning::new(true));

        service(repo.clone(), provisioning.clone()).run_if_due();

        assert_eq!(provisioning.update_count(), 1);
        let jobs = repo.all();
        assert!(
            jobs.iter().any(|job| job.status == JobStatus::Failed),
            "expected a FAILED tiles_update job"
        );
    }

    #[test]
    fn records_cancelled_when_a_request_arrives_during_the_build() {
        // A RUNNING job is created by execute(); simulate a cancellation request
        // landing while the (atomic) build runs by pre-requesting it via the
        // repository's in-memory lock + status before execute's finalize step.
        let repo = Arc::new(MemoryJobRepository::new(vec![]));
        let provisioning = Arc::new(MockTilesProvisioning::new(false));
        let svc = service(repo.clone(), provisioning.clone());

        // Seed a RUNNING job that is then requested for cancellation, and run
        // the finalize-after-update path directly against it.
        let job = running_job(Utc::now());
        repo.insert(job.clone()).unwrap();
        repo.request_cancellation(job.id).unwrap();

        svc.finalize_after_update(job.id, &job.name);

        let stored = repo.find_by_id(job.id).unwrap().unwrap();
        assert_eq!(stored.status, JobStatus::Cancelled);
    }

    #[test]
    fn does_not_claim_when_another_instance_holds_the_lock() {
        let repo = Arc::new(MemoryJobRepository::new(vec![]));
        // Another instance already owns the lock.
        repo.acquire(
            TILES_UPDATE_JOB_TYPE,
            Uuid::new_v4(),
            Utc::now() + Duration::hours(1),
        )
        .unwrap();
        let provisioning = Arc::new(MockTilesProvisioning::new(false));

        service(repo.clone(), provisioning.clone()).run_if_due();

        assert_eq!(provisioning.update_count(), 0);
    }
}
