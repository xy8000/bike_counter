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

use std::str::FromStr;
use std::sync::Arc;

use chrono::{DateTime, Utc};
use uuid::Uuid;

use crate::core::domain::configuration::configuration::Configuration;
use crate::core::domain::error::DomainError;
use crate::core::domain::jobs::job::Job;
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
}

impl TilesUpdateService {
    pub fn new(
        job_repository: Arc<dyn JobRepository + Send + Sync>,
        tiles_provisioning: Arc<dyn TilesProvisioningPort>,
        configuration: Arc<Configuration>,
    ) -> Self {
        Self {
            job_repository,
            tiles_provisioning,
            configuration,
        }
    }

    /// Decides whether the tiles update job should run now and executes it if
    /// so. Same always-on rule as the data-source update and asset cleanup jobs:
    /// run at startup (never succeeded) and whenever the last successful run is
    /// overdue; skip while a RUNNING job is still within its lifetime.
    pub fn run_if_due(&self) {
        let now = Utc::now();

        match self
            .job_repository
            .expire_running_jobs(TILES_UPDATE_JOB_TYPE, now)
        {
            Ok(expired) if expired > 0 => {
                println!("Expired {expired} stale RUNNING tiles update job(s)");
            }
            Ok(_) => {}
            Err(error) => {
                eprintln!("Failed to expire stale tiles update jobs: {error:?}");
            }
        }

        match self
            .job_repository
            .find_running_by_type(TILES_UPDATE_JOB_TYPE)
        {
            Ok(Some(running)) if !running.lifetime_exceeded(now) => {
                println!(
                    "Tiles update job {} is still running (until {}); skipping",
                    running.id, running.lifetime_until
                );
                return;
            }
            Ok(_) => {}
            Err(error) => {
                eprintln!("Failed to check for a running tiles update job: {error:?}");
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
                        "Tiles update job is overdue (last run at {}); running",
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

    /// Runs one tiles update as a tracked job.
    fn execute(&self, now: DateTime<Utc>) {
        let job = Job::new(
            Uuid::new_v4(),
            TILES_UPDATE_JOB_NAME.to_string(),
            TILES_UPDATE_JOB_TYPE.to_string(),
            now + self.configuration.maps().update_max_lifetime(),
        );
        let job_id = job.id;
        let job_name = job.name.clone();

        if let Err(error) = self.job_repository.insert(job) {
            eprintln!("Failed to record tiles update job {job_name} ({job_id}): {error:?}");
            return;
        }

        if let Err(error) = self.job_repository.set_running(job_id, now) {
            let message = format!("failed to start job: {error:?}");
            if let Err(fail_error) = self.job_repository.set_failed(job_id, Utc::now(), &message) {
                eprintln!(
                    "Failed to mark tiles update job {job_name} ({job_id}) as failed: {fail_error:?}"
                );
            }
            return;
        }

        println!("Tiles update job {job_name} ({job_id}) started");

        match self.run_update() {
            Ok(()) => {
                if let Err(error) = self.job_repository.set_finished(job_id, Utc::now()) {
                    eprintln!("Failed to finish tiles update job {job_name} ({job_id}): {error:?}");
                } else {
                    println!("Tiles update job {job_name} ({job_id}) finished");
                }
            }
            Err(error) => {
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
            }
        }
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
    use std::sync::{Arc, Mutex};

    use chrono::{Duration, Utc};
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
    use crate::core::domain::jobs::job::JobStatus;

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
                    "https://build.protomaps.com/20260829.pmtiles".to_string(),
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
    }

    impl MemoryJobRepository {
        fn new(jobs: Vec<Job>) -> Self {
            Self {
                jobs: Mutex::new(jobs),
            }
        }

        fn all(&self) -> Vec<Job> {
            self.jobs.lock().unwrap().clone()
        }
    }

    impl JobRepository for MemoryJobRepository {
        fn insert(&self, job: Job) -> Result<(), DomainError> {
            self.jobs.lock().unwrap().push(job);
            Ok(())
        }
        fn set_running(&self, id: Uuid, started_at: DateTime<Utc>) -> Result<(), DomainError> {
            let mut jobs = self.jobs.lock().unwrap();
            if let Some(job) = jobs.iter_mut().find(|job| job.id == id) {
                job.status = JobStatus::Running;
                job.started_at = Some(started_at);
            }
            Ok(())
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
        fn find_running_by_type(&self, job_type: &str) -> Result<Option<Job>, DomainError> {
            Ok(self
                .jobs
                .lock()
                .unwrap()
                .iter()
                .find(|job| job.job_type == job_type && job.status == JobStatus::Running)
                .cloned())
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
        fn expire_running_jobs(
            &self,
            job_type: &str,
            now: DateTime<Utc>,
        ) -> Result<u64, DomainError> {
            let mut jobs = self.jobs.lock().unwrap();
            let mut expired = 0u64;
            for job in jobs.iter_mut() {
                if job.job_type == job_type
                    && job.status == JobStatus::Running
                    && job.lifetime_exceeded(now)
                {
                    job.status = JobStatus::Failed;
                    job.max_lifetime_exceeded = true;
                    job.failure_message = Some("max lifetime exceeded".to_string());
                    expired += 1;
                }
            }
            Ok(expired)
        }
    }

    fn finished_job(now: DateTime<Utc>) -> Job {
        let mut job = Job::new(
            Uuid::new_v4(),
            TILES_UPDATE_JOB_NAME.to_string(),
            TILES_UPDATE_JOB_TYPE.to_string(),
            now + Duration::hours(1),
        );
        job.status = JobStatus::Finished;
        job.finished_at = Some(now);
        job
    }

    fn running_job(now: DateTime<Utc>) -> Job {
        let mut job = Job::new(
            Uuid::new_v4(),
            TILES_UPDATE_JOB_NAME.to_string(),
            TILES_UPDATE_JOB_TYPE.to_string(),
            now + Duration::hours(1),
        );
        job.status = JobStatus::Running;
        job.started_at = Some(now);
        job
    }

    #[test]
    fn runs_when_never_succeeded() {
        let repo = Arc::new(MemoryJobRepository::new(vec![]));
        let provisioning = Arc::new(MockTilesProvisioning::new(false));

        let service = TilesUpdateService::new(repo.clone(), provisioning.clone(), configuration());
        service.run_if_due();

        assert_eq!(provisioning.update_count(), 1);
    }

    #[test]
    fn skips_while_a_running_job_is_within_its_lifetime() {
        let now = Utc::now();
        let repo = Arc::new(MemoryJobRepository::new(vec![running_job(now)]));
        let provisioning = Arc::new(MockTilesProvisioning::new(false));

        let service = TilesUpdateService::new(repo.clone(), provisioning.clone(), configuration());
        service.run_if_due();

        assert_eq!(provisioning.update_count(), 0);
    }

    #[test]
    fn runs_when_overdue_and_records_finished() {
        let now = Utc::now();
        let repo = Arc::new(MemoryJobRepository::new(vec![finished_job(
            now - Duration::days(200),
        )]));
        let provisioning = Arc::new(MockTilesProvisioning::new(false));

        let service = TilesUpdateService::new(repo.clone(), provisioning.clone(), configuration());
        service.run_if_due();

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
        // Finished a day ago: the bi-monthly cron is not due.
        let repo = Arc::new(MemoryJobRepository::new(vec![finished_job(
            now - Duration::days(1),
        )]));
        let provisioning = Arc::new(MockTilesProvisioning::new(false));

        let service = TilesUpdateService::new(repo.clone(), provisioning.clone(), configuration());
        service.run_if_due();

        assert_eq!(provisioning.update_count(), 0);
    }

    #[test]
    fn records_failure_when_the_update_fails() {
        let repo = Arc::new(MemoryJobRepository::new(vec![]));
        let provisioning = Arc::new(MockTilesProvisioning::new(true));

        let service = TilesUpdateService::new(repo.clone(), provisioning.clone(), configuration());
        service.run_if_due();

        assert_eq!(provisioning.update_count(), 1);
        let jobs = repo.all();
        assert!(
            jobs.iter().any(|job| job.status == JobStatus::Failed),
            "expected a FAILED tiles_update job"
        );
    }
}
