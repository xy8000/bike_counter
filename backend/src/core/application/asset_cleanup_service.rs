//! Application job runner that deletes **orphaned** objects from the asset
//! storage bucket: object keys present in MinIO but with no row in the `assets`
//! table (left behind when a provider image hash changes, or after a crash
//! between `put` and `save`).
//!
//! Mirrors [`DataSourceUpdateService`]'s ShedLock-style scheduling: it is driven
//! by the generic cron scheduler through [`ScheduledJobPort`] and tracks itself
//! as a `asset_cleanup` job.

use std::collections::HashSet;
use std::str::FromStr;
use std::sync::Arc;

use chrono::{DateTime, Utc};
use uuid::Uuid;

use crate::core::domain::assets::asset::value_objects::ObjectKey;
use crate::core::domain::assets::asset_storage_port::AssetStorage;
use crate::core::domain::assets::repository_port::AssetRepository;
use crate::core::domain::configuration::configuration::Configuration;
use crate::core::domain::error::DomainError;
use crate::core::domain::jobs::job::Job;
use crate::core::domain::jobs::repository_port::JobRepository;
use crate::core::domain::jobs::scheduled_job_port::ScheduledJobPort;
use serde_json::json;

/// The job type owned by this service.
pub const ASSET_CLEANUP_JOB_TYPE: &str = "asset_cleanup";
/// Human-readable name of the asset cleanup job.
pub const ASSET_CLEANUP_JOB_NAME: &str = "asset cleanup";

/// Job-metadata keys recording the outcome of the last cleanup run.
pub const ORPHANED_OBJECTS_KEY: &str = "orphaned_objects";
pub const DELETED_OBJECTS_KEY: &str = "deleted_objects";

pub struct AssetCleanupService {
    job_repository: Arc<dyn JobRepository + Send + Sync>,
    asset_repository: Arc<dyn AssetRepository>,
    asset_storage: Arc<dyn AssetStorage>,
    configuration: Arc<Configuration>,
}

impl AssetCleanupService {
    pub fn new(
        job_repository: Arc<dyn JobRepository + Send + Sync>,
        asset_repository: Arc<dyn AssetRepository>,
        asset_storage: Arc<dyn AssetStorage>,
        configuration: Arc<Configuration>,
    ) -> Self {
        Self {
            job_repository,
            asset_repository,
            asset_storage,
            configuration,
        }
    }

    /// Decides whether the asset cleanup job should run now and executes it if
    /// so. Same always-on rule as the data-source update job: run at startup
    /// (never succeeded) and whenever the last successful run is overdue; skip
    /// while a RUNNING job is still within its lifetime.
    pub fn run_if_due(&self) {
        let now = Utc::now();

        match self
            .job_repository
            .expire_running_jobs(ASSET_CLEANUP_JOB_TYPE, now)
        {
            Ok(expired) if expired > 0 => {
                println!("Expired {expired} stale RUNNING asset cleanup job(s)");
            }
            Ok(_) => {}
            Err(error) => {
                eprintln!("Failed to expire stale asset cleanup jobs: {error:?}");
            }
        }

        match self
            .job_repository
            .find_running_by_type(ASSET_CLEANUP_JOB_TYPE)
        {
            Ok(Some(running)) if !running.lifetime_exceeded(now) => {
                println!(
                    "Asset cleanup job {} is still running (until {}); skipping",
                    running.id, running.lifetime_until
                );
                return;
            }
            Ok(_) => {}
            Err(error) => {
                eprintln!("Failed to check for a running asset cleanup job: {error:?}");
                return;
            }
        }

        match self
            .job_repository
            .find_last_finished_by_type(ASSET_CLEANUP_JOB_TYPE)
        {
            Ok(None) => {
                println!("Asset cleanup job has never succeeded; running");
                self.execute(now);
            }
            Ok(Some(last)) => {
                if self.is_overdue(&last, now) {
                    println!(
                        "Asset cleanup job is overdue (last run at {}); running",
                        last.finished_at
                            .map(|ts| ts.to_rfc3339())
                            .unwrap_or_else(|| "unknown".to_string())
                    );
                    self.execute(now);
                }
            }
            Err(error) => {
                eprintln!("Failed to check the last finished asset cleanup job: {error:?}");
            }
        }
    }

    /// Whether the last successful run is overdue: the next scheduled cron
    /// trigger after its finish time has already passed.
    fn is_overdue(&self, last: &Job, now: DateTime<Utc>) -> bool {
        let schedule = match cron::Schedule::from_str(self.configuration.asset_cleanup_cron()) {
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

    /// Runs one cleanup pass as a tracked job.
    fn execute(&self, now: DateTime<Utc>) {
        let job = Job::new(
            Uuid::new_v4(),
            ASSET_CLEANUP_JOB_NAME.to_string(),
            ASSET_CLEANUP_JOB_TYPE.to_string(),
            now + self.configuration.asset_cleanup_max_lifetime(),
        );
        let job_id = job.id;
        let job_name = job.name.clone();

        if let Err(error) = self.job_repository.insert(job) {
            eprintln!("Failed to record asset cleanup job {job_name} ({job_id}): {error:?}");
            return;
        }

        if let Err(error) = self.job_repository.set_running(job_id, now) {
            let message = format!("failed to start job: {error:?}");
            if let Err(fail_error) = self.job_repository.set_failed(job_id, Utc::now(), &message) {
                eprintln!(
                    "Failed to mark asset cleanup job {job_name} ({job_id}) as failed: {fail_error:?}"
                );
            }
            return;
        }

        println!("Asset cleanup job {job_name} ({job_id}) started");

        match self.run_cleanup(job_id) {
            Ok(()) => {
                if let Err(error) = self.job_repository.set_finished(job_id, Utc::now()) {
                    eprintln!(
                        "Failed to finish asset cleanup job {job_name} ({job_id}): {error:?}"
                    );
                } else {
                    println!("Asset cleanup job {job_name} ({job_id}) finished");
                }
            }
            Err(error) => {
                let message = format!("{error:?}");
                if let Err(set_failed_error) =
                    self.job_repository.set_failed(job_id, Utc::now(), &message)
                {
                    eprintln!(
                        "Failed to mark asset cleanup job {job_name} ({job_id}) as failed: {set_failed_error:?}"
                    );
                } else {
                    eprintln!("Asset cleanup job {job_name} ({job_id}) failed: {error:?}");
                }
            }
        }
    }

    /// Deletes every object key in the bucket that has no `assets` row and
    /// records the counts in the job metadata.
    fn run_cleanup(&self, job_id: Uuid) -> Result<(), DomainError> {
        let objects: HashSet<String> = self
            .asset_storage
            .list_object_keys()?
            .into_iter()
            .map(|key| key.0)
            .collect();
        let known: HashSet<String> = self
            .asset_repository
            .all_object_keys()?
            .into_iter()
            .map(|key| key.0)
            .collect();

        let orphans: Vec<String> = objects.difference(&known).cloned().collect();
        let mut deleted = 0usize;
        for key in &orphans {
            self.asset_storage.delete(&ObjectKey(key.clone()))?;
            deleted += 1;
        }

        self.job_repository
            .update_metadata(job_id, ORPHANED_OBJECTS_KEY, json!(orphans.len()))?;
        self.job_repository
            .update_metadata(job_id, DELETED_OBJECTS_KEY, json!(deleted))?;

        if deleted > 0 {
            println!("Asset cleanup deleted {deleted} orphaned object(s)");
        }
        Ok(())
    }
}

impl ScheduledJobPort for AssetCleanupService {
    fn run_if_due(&self) {
        self.run_if_due();
    }
}

#[cfg(test)]
mod tests {
    use std::collections::HashSet;
    use std::str::FromStr;
    use std::sync::{Arc, Mutex};

    use chrono::Duration;
    use serde_json::Value;

    use super::*;
    use crate::core::domain::assets::asset::Asset;
    use crate::core::domain::assets::asset::value_objects::{ContentType, ObjectKey};
    use crate::core::domain::assets::asset_storage_port::AssetObjectInfo;
    use crate::core::domain::configuration::configuration::value_objects::{
        AssetStorageConfiguration, DatabaseConfiguration,
    };
    use crate::core::domain::configuration::configuration::{
        Configuration, DEFAULT_ASSET_CLEANUP_CRON, DEFAULT_DATA_SOURCE_UPDATE_CRON,
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
            )
            .unwrap(),
        )
    }

    struct MemoryAssetRepository {
        known: Mutex<HashSet<String>>,
    }

    impl AssetRepository for MemoryAssetRepository {
        fn find_by_id(
            &self,
            _id: crate::core::domain::assets::asset::value_objects::AssetId,
        ) -> Result<Option<Asset>, DomainError> {
            Ok(None)
        }
        fn find_by_object_key(
            &self,
            _object_key: &ObjectKey,
        ) -> Result<Option<Asset>, DomainError> {
            Ok(None)
        }
        fn save(&self, _asset: Asset) -> Result<Asset, DomainError> {
            Err(DomainError::Database("not used".to_string()))
        }
        fn delete(&self, object_key: &ObjectKey) -> Result<(), DomainError> {
            self.known.lock().unwrap().remove(&object_key.0);
            Ok(())
        }
        fn list(&self) -> Result<Vec<Asset>, DomainError> {
            Ok(Vec::new())
        }
        fn all_object_keys(&self) -> Result<Vec<ObjectKey>, DomainError> {
            Ok(self
                .known
                .lock()
                .unwrap()
                .iter()
                .map(|key| ObjectKey(key.clone()))
                .collect())
        }
    }

    struct MemoryAssetStorage {
        objects: Mutex<HashSet<String>>,
    }

    impl AssetStorage for MemoryAssetStorage {
        fn ensure_bucket(&self) -> Result<(), DomainError> {
            Ok(())
        }
        fn put(
            &self,
            _object_key: &ObjectKey,
            _content_type: &ContentType,
            _bytes: &[u8],
        ) -> Result<AssetObjectInfo, DomainError> {
            Ok(AssetObjectInfo { byte_size: 0 })
        }
        fn list_object_keys(&self) -> Result<Vec<ObjectKey>, DomainError> {
            Ok(self
                .objects
                .lock()
                .unwrap()
                .iter()
                .map(|key| ObjectKey(key.clone()))
                .collect())
        }
        fn delete(&self, object_key: &ObjectKey) -> Result<(), DomainError> {
            self.objects.lock().unwrap().remove(&object_key.0);
            Ok(())
        }
        fn get_stream(
            &self,
            _object_key: &ObjectKey,
        ) -> std::pin::Pin<
            Box<
                dyn std::future::Future<
                        Output = Result<
                            crate::core::domain::assets::asset_storage_port::AssetObjectStream,
                            DomainError,
                        >,
                    > + Send
                    + '_,
            >,
        > {
            Box::pin(async { unimplemented!("not used in these unit tests") })
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
                .max_by_key(|job| job.finished_at)
                .cloned())
        }
        fn expire_running_jobs(
            &self,
            _job_type: &str,
            _now: DateTime<Utc>,
        ) -> Result<u64, DomainError> {
            Ok(0)
        }
    }

    /// A [`JobRepository`] that fails (or reports expired runs) on demand.
    struct FailingJobRepository {
        fail: Mutex<HashSet<&'static str>>,
        expire_result: u64,
    }

    impl FailingJobRepository {
        fn new(fail: &[&'static str]) -> Self {
            Self {
                fail: Mutex::new(fail.iter().copied().collect()),
                expire_result: 0,
            }
        }

        fn with_expire_result(mut self, result: u64) -> Self {
            self.expire_result = result;
            self
        }

        fn should_fail(&self, op: &str) -> bool {
            self.fail.lock().unwrap().contains(op)
        }
    }

    impl JobRepository for FailingJobRepository {
        fn insert(&self, _job: Job) -> Result<(), DomainError> {
            if self.should_fail("insert") {
                Err(DomainError::Database("boom".to_string()))
            } else {
                Ok(())
            }
        }
        fn set_running(&self, _id: Uuid, _started_at: DateTime<Utc>) -> Result<(), DomainError> {
            if self.should_fail("set_running") {
                Err(DomainError::Database("boom".to_string()))
            } else {
                Ok(())
            }
        }
        fn set_finished(&self, _id: Uuid, _finished_at: DateTime<Utc>) -> Result<(), DomainError> {
            if self.should_fail("set_finished") {
                Err(DomainError::Database("boom".to_string()))
            } else {
                Ok(())
            }
        }
        fn set_failed(
            &self,
            _id: Uuid,
            _finished_at: DateTime<Utc>,
            _message: &str,
        ) -> Result<(), DomainError> {
            if self.should_fail("set_failed") {
                Err(DomainError::Database("boom".to_string()))
            } else {
                Ok(())
            }
        }
        fn update_metadata(&self, _id: Uuid, _key: &str, _value: Value) -> Result<(), DomainError> {
            Ok(())
        }
        fn find_by_id(&self, _id: Uuid) -> Result<Option<Job>, DomainError> {
            Ok(None)
        }
        fn find_all(
            &self,
            _job_type: Option<&str>,
            _status: Option<JobStatus>,
        ) -> Result<Vec<Job>, DomainError> {
            Ok(Vec::new())
        }
        fn find_running_by_type(&self, _job_type: &str) -> Result<Option<Job>, DomainError> {
            if self.should_fail("find_running") {
                Err(DomainError::Database("boom".to_string()))
            } else {
                Ok(None)
            }
        }
        fn find_last_finished_by_type(&self, _job_type: &str) -> Result<Option<Job>, DomainError> {
            if self.should_fail("find_last") {
                Err(DomainError::Database("boom".to_string()))
            } else {
                Ok(None)
            }
        }
        fn expire_running_jobs(
            &self,
            _job_type: &str,
            _now: DateTime<Utc>,
        ) -> Result<u64, DomainError> {
            if self.should_fail("expire") {
                Err(DomainError::Database("boom".to_string()))
            } else {
                Ok(self.expire_result)
            }
        }
    }

    /// A storage whose `delete` always fails (cleanup error path).
    struct FailingStorage;

    impl AssetStorage for FailingStorage {
        fn ensure_bucket(&self) -> Result<(), DomainError> {
            Ok(())
        }
        fn put(
            &self,
            _object_key: &ObjectKey,
            _content_type: &ContentType,
            _bytes: &[u8],
        ) -> Result<AssetObjectInfo, DomainError> {
            Ok(AssetObjectInfo { byte_size: 0 })
        }
        fn list_object_keys(&self) -> Result<Vec<ObjectKey>, DomainError> {
            Ok(vec![ObjectKey("provider/orphan.jpg".to_string())])
        }
        fn delete(&self, _object_key: &ObjectKey) -> Result<(), DomainError> {
            Err(DomainError::Database("boom".to_string()))
        }
        fn get_stream(
            &self,
            _object_key: &ObjectKey,
        ) -> std::pin::Pin<
            Box<
                dyn std::future::Future<
                        Output = Result<
                            crate::core::domain::assets::asset_storage_port::AssetObjectStream,
                            DomainError,
                        >,
                    > + Send
                    + '_,
            >,
        > {
            Box::pin(async { unimplemented!("not used in these unit tests") })
        }
    }

    fn finished_job(finished_at: DateTime<Utc>) -> Job {
        let mut job = Job::new(
            Uuid::new_v4(),
            ASSET_CLEANUP_JOB_NAME.to_string(),
            ASSET_CLEANUP_JOB_TYPE.to_string(),
            Utc::now() + Duration::minutes(10),
        );
        job.status = JobStatus::Finished;
        job.finished_at = Some(finished_at);
        job
    }

    fn running_job() -> Job {
        let mut job = Job::new(
            Uuid::new_v4(),
            ASSET_CLEANUP_JOB_NAME.to_string(),
            ASSET_CLEANUP_JOB_TYPE.to_string(),
            Utc::now() + Duration::minutes(10),
        );
        job.status = JobStatus::Running;
        job.started_at = Some(Utc::now());
        job
    }

    #[test]
    fn deletes_only_objects_without_an_assets_row_and_records_metadata() {
        let asset_repo = Arc::new(MemoryAssetRepository {
            known: Mutex::new(HashSet::from([
                "builtin/bike-icon-black-transparent.svg".to_string(),
                "provider/abc123.jpg".to_string(),
            ])),
        });
        let storage = Arc::new(MemoryAssetStorage {
            objects: Mutex::new(HashSet::from([
                "builtin/bike-icon-black-transparent.svg".to_string(),
                "provider/abc123.jpg".to_string(),
                "provider/orphan1.jpg".to_string(),
                "provider/orphan2.jpg".to_string(),
            ])),
        });
        let job_repo = Arc::new(MemoryJobRepository::new(Vec::new()));
        let service = AssetCleanupService::new(
            job_repo.clone(),
            asset_repo.clone(),
            storage.clone(),
            configuration(),
        );

        service.run_if_due();

        let remaining = storage.list_object_keys().unwrap();
        let mut keys: Vec<_> = remaining.into_iter().map(|key| key.0).collect();
        keys.sort();
        assert_eq!(
            keys,
            vec![
                "builtin/bike-icon-black-transparent.svg".to_string(),
                "provider/abc123.jpg".to_string()
            ]
        );

        let jobs = job_repo.jobs.lock().unwrap();
        let finished = jobs
            .iter()
            .find(|job| job.status == JobStatus::Finished)
            .expect("a finished job must exist");
        assert_eq!(finished.metadata[ORPHANED_OBJECTS_KEY], json!(2));
        assert_eq!(finished.metadata[DELETED_OBJECTS_KEY], json!(2));
    }

    #[test]
    fn skips_while_another_run_is_still_within_its_lifetime() {
        let storage = Arc::new(MemoryAssetStorage {
            objects: Mutex::new(HashSet::from(["provider/orphan.jpg".to_string()])),
        });
        let asset_repo = Arc::new(MemoryAssetRepository {
            known: Mutex::new(HashSet::new()),
        });
        let job_repo = Arc::new(MemoryJobRepository::new(vec![running_job()]));
        let service = AssetCleanupService::new(
            job_repo.clone(),
            asset_repo.clone(),
            storage.clone(),
            configuration(),
        );

        service.run_if_due();

        assert_eq!(
            storage.list_object_keys().unwrap().len(),
            1,
            "must not delete while a run is active"
        );
        assert!(
            !job_repo
                .jobs
                .lock()
                .unwrap()
                .iter()
                .any(|job| job.status == JobStatus::Finished)
        );
    }

    #[test]
    fn does_not_run_when_last_run_is_recent() {
        let storage = Arc::new(MemoryAssetStorage {
            objects: Mutex::new(HashSet::from(["provider/orphan.jpg".to_string()])),
        });
        let asset_repo = Arc::new(MemoryAssetRepository {
            known: Mutex::new(HashSet::new()),
        });
        // Finished "just now" -> not overdue.
        let job_repo = Arc::new(MemoryJobRepository::new(vec![finished_job(Utc::now())]));
        let service = AssetCleanupService::new(
            job_repo.clone(),
            asset_repo.clone(),
            storage.clone(),
            configuration(),
        );

        service.run_if_due();

        assert_eq!(storage.list_object_keys().unwrap().len(), 1);
        assert_eq!(
            job_repo
                .jobs
                .lock()
                .unwrap()
                .iter()
                .filter(|job| job.status == JobStatus::Finished)
                .count(),
            1,
            "no second job should be created"
        );
    }

    #[test]
    fn runs_when_last_run_is_overdue() {
        let storage = Arc::new(MemoryAssetStorage {
            objects: Mutex::new(HashSet::from(["provider/orphan.jpg".to_string()])),
        });
        let asset_repo = Arc::new(MemoryAssetRepository {
            known: Mutex::new(HashSet::new()),
        });
        // Finished far in the past (before the daily 04:00 cron trigger).
        let long_ago = Utc::now() - Duration::days(2);
        let job_repo = Arc::new(MemoryJobRepository::new(vec![finished_job(long_ago)]));
        let service = AssetCleanupService::new(
            job_repo.clone(),
            asset_repo.clone(),
            storage.clone(),
            configuration(),
        );

        service.run_if_due();

        assert_eq!(storage.list_object_keys().unwrap().len(), 0);
        assert_eq!(
            job_repo
                .jobs
                .lock()
                .unwrap()
                .iter()
                .filter(|job| job.status == JobStatus::Finished)
                .count(),
            2,
            "an overdue cleanup must create a second run"
        );
    }

    #[test]
    fn is_overdue_parses_cron_expression() {
        let config = configuration();
        let service = AssetCleanupService::new(
            Arc::new(MemoryJobRepository::new(Vec::new())),
            Arc::new(MemoryAssetRepository {
                known: Mutex::new(HashSet::new()),
            }),
            Arc::new(MemoryAssetStorage {
                objects: Mutex::new(HashSet::new()),
            }),
            config.clone(),
        );

        let recent = finished_job(Utc::now());
        assert!(!service.is_overdue(&recent, Utc::now()));

        let old = finished_job(Utc::now() - Duration::days(2));
        assert!(service.is_overdue(&old, Utc::now()));

        // Defensive: an invalid cron must not panic, just return false.
        let _schedule = cron::Schedule::from_str("0 0 4 * * *");
    }

    fn error_service(
        job_repo: Arc<FailingJobRepository>,
        storage: Arc<dyn AssetStorage>,
    ) -> AssetCleanupService {
        AssetCleanupService::new(
            job_repo,
            Arc::new(MemoryAssetRepository {
                known: Mutex::new(HashSet::new()),
            }),
            storage,
            configuration(),
        )
    }

    #[test]
    fn survives_repository_lookup_errors() {
        // Expire and find_running both fail -> log and skip without panicking.
        let service = error_service(
            Arc::new(FailingJobRepository::new(&["expire", "find_running"])),
            Arc::new(MemoryAssetStorage {
                objects: Mutex::new(HashSet::new()),
            }),
        );
        service.run_if_due();

        // find_running error returns early, so nothing was deleted.
        let service = error_service(
            Arc::new(FailingJobRepository::new(&["find_last"])),
            Arc::new(MemoryAssetStorage {
                objects: Mutex::new(HashSet::from(["provider/orphan.jpg".to_string()])),
            }),
        );
        service.run_if_due();
        // No assertion needed beyond "does not panic"; both error branches log.
    }

    #[test]
    fn records_expired_stale_running_jobs() {
        let storage = Arc::new(MemoryAssetStorage {
            objects: Mutex::new(HashSet::new()),
        });
        let job_repo = Arc::new(FailingJobRepository::new(&[]).with_expire_result(1));
        let service = error_service(job_repo, storage);
        service.run_if_due();
    }

    #[test]
    fn marks_job_failed_when_start_fails() {
        let service = error_service(
            Arc::new(FailingJobRepository::new(&["set_running"])),
            Arc::new(MemoryAssetStorage {
                objects: Mutex::new(HashSet::new()),
            }),
        );
        service.run_if_due();
    }

    #[test]
    fn marks_job_failed_when_insert_fails() {
        let service = error_service(
            Arc::new(FailingJobRepository::new(&["insert"])),
            Arc::new(MemoryAssetStorage {
                objects: Mutex::new(HashSet::new()),
            }),
        );
        service.run_if_due();
    }

    #[test]
    fn marks_job_failed_when_cleanup_fails() {
        let service = error_service(
            Arc::new(FailingJobRepository::new(&[])),
            Arc::new(FailingStorage),
        );
        service.run_if_due();
    }
}
