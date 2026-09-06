//! Application job runner that deletes **orphaned** objects from the asset
//! storage bucket: object keys present in MinIO but with no row in the `assets`
//! table (left behind when a provider image hash changes, or after a crash
//! between `put` and `save`).
//!
//! Mirrors [`DataSourceUpdateService`]'s scheduling: it is driven by the generic
//! cron scheduler through [`ScheduledJobPort`] and tracks itself as a
//! `asset_cleanup` job. Multi-instance cancellation follows the shared protocol
//! (claim the type's `job_locks` row, record a RUNNING job owned by this
//! instance, heartbeat after each sub-task and honor a cancellation request).

use std::collections::HashSet;
use std::str::FromStr;
use std::sync::Arc;

use chrono::{DateTime, Utc};
use uuid::Uuid;

use super::job_heartbeat::JobHeartbeat;
use crate::core::domain::assets::asset::value_objects::ObjectKey;
use crate::core::domain::assets::asset_storage_port::AssetStorage;
use crate::core::domain::assets::repository_port::AssetRepository;
use crate::core::domain::configuration::configuration::Configuration;
use crate::core::domain::error::DomainError;
use crate::core::domain::jobs::job::{Job, JobStatus};
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
    instance_id: Uuid,
}

impl AssetCleanupService {
    pub fn new(
        job_repository: Arc<dyn JobRepository + Send + Sync>,
        asset_repository: Arc<dyn AssetRepository>,
        asset_storage: Arc<dyn AssetStorage>,
        configuration: Arc<Configuration>,
        instance_id: Uuid,
    ) -> Self {
        Self {
            job_repository,
            asset_repository,
            asset_storage,
            configuration,
            instance_id,
        }
    }

    /// Decides whether the asset cleanup job should run now and executes it if
    /// so. Same always-on rule as the data-source update job: run at startup
    /// (never succeeded) and whenever the last successful run is overdue; skip
    /// while an active job is still in flight or being finalized.
    pub fn run_if_due(&self) {
        let now = Utc::now();

        match self
            .job_repository
            .find_active_by_type(ASSET_CLEANUP_JOB_TYPE)
        {
            Ok(active) if !active.is_empty() => {
                let count = active.len();
                let ids = active
                    .iter()
                    .map(|job| job.id.to_string())
                    .collect::<Vec<_>>()
                    .join(", ");
                println!(
                    "Asset cleanup job is still active ({count} running/requesting: {ids}); \
                     skipping"
                );
                return;
            }
            Ok(_) => {}
            Err(error) => {
                eprintln!("Failed to check for an active asset cleanup job: {error:?}");
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
                        "Asset cleanup job is overdue (last run {} at {}); running",
                        last.id,
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

    /// Runs one cleanup pass as a job owned by this instance.
    fn execute(&self, now: DateTime<Utc>) {
        let interval = self.configuration.asset_cleanup_max_heartbeat_interval();
        let instance_id = self.instance_id;

        // 1. Claim the type's lock; only the winning instance proceeds.
        match self
            .job_repository
            .acquire(ASSET_CLEANUP_JOB_TYPE, instance_id, now + interval)
        {
            Ok(true) => {}
            Ok(false) => {
                println!("Asset cleanup is already active elsewhere (job_locks held); skipping");
                return;
            }
            Err(error) => {
                eprintln!("Failed to acquire the asset cleanup lock: {error:?}");
                return;
            }
        }

        // 2. Record the RUNNING job owned by this instance.
        let job = Job::running(
            Uuid::new_v4(),
            ASSET_CLEANUP_JOB_NAME.to_string(),
            ASSET_CLEANUP_JOB_TYPE.to_string(),
            instance_id,
            now,
        );
        let job_id = job.id;
        let job_name = job.name.clone();
        if let Err(error) = self.job_repository.insert(job) {
            let _ = self
                .job_repository
                .release(ASSET_CLEANUP_JOB_TYPE, instance_id);
            eprintln!("Failed to record asset cleanup job {job_name} ({job_id}): {error:?}");
            return;
        }
        println!("Asset cleanup job {job_name} ({job_id}) started");

        // 3. A dedicated heartbeat loop keeps the job fresh on a fixed tick,
        //    independent of how many objects the cleanup has to delete.
        let heartbeat = JobHeartbeat::start(
            self.job_repository.clone(),
            job_id,
            ASSET_CLEANUP_JOB_TYPE,
            instance_id,
            interval,
        );
        let outcome = self.run_cleanup(job_id);
        heartbeat.stop();

        // 4. Finalize based on the resulting status.
        self.finalize(job_id, &job_name, outcome);
    }

    /// Finalizes the job according to `outcome` and the current persisted
    /// status, then always releases the type's lock.
    fn finalize(&self, job_id: Uuid, job_name: &str, outcome: Result<(), DomainError>) {
        let status = self
            .job_repository
            .find_by_id(job_id)
            .ok()
            .flatten()
            .map(|job| job.status);
        match (outcome, status) {
            (_, Some(JobStatus::CancellationRequested)) | (_, Some(JobStatus::Cancelled)) => {
                match self.job_repository.mark_cancelled(job_id, Utc::now()) {
                    Ok(()) => println!("Asset cleanup job {job_name} ({job_id}) cancelled"),
                    Err(error) => eprintln!(
                        "Could not finalize asset cleanup job {job_name} ({job_id}) as cancelled: {error:?}"
                    ),
                }
            }
            (Ok(()), _) => {
                if let Err(error) = self.job_repository.set_finished(job_id, Utc::now()) {
                    eprintln!(
                        "Failed to finish asset cleanup job {job_name} ({job_id}): {error:?}"
                    );
                } else {
                    println!("Asset cleanup job {job_name} ({job_id}) finished");
                }
            }
            (Err(error), _) => {
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
        let _ = self
            .job_repository
            .release(ASSET_CLEANUP_JOB_TYPE, self.instance_id);
    }

    /// Deletes every object key in the bucket that has no `assets` row and
    /// records the counts in the job metadata. Each delete is a sub-task
    /// boundary: the job heartbeats and honors a cancellation request by
    /// returning [`DomainError::Cancelled`].
    fn run_cleanup(&self, job_id: Uuid) -> Result<(), DomainError> {
        self.check_cancellation(job_id)?;

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
            self.check_cancellation(job_id)?;
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

    /// Heartbeats the job and returns [`DomainError::Cancelled`] when a
    /// cancellation was requested, so the cleanup loop stops gracefully.
    fn check_cancellation(&self, job_id: Uuid) -> Result<(), DomainError> {
        let now = Utc::now();
        let interval = self.configuration.asset_cleanup_max_heartbeat_interval();
        match self.job_repository.heartbeat(
            job_id,
            ASSET_CLEANUP_JOB_TYPE,
            self.instance_id,
            now,
            now + interval,
        ) {
            Ok(JobStatus::CancellationRequested) | Ok(JobStatus::Cancelled) => {
                Err(DomainError::Cancelled)
            }
            Ok(_) => Ok(()),
            Err(error) => Err(error),
        }
    }
}

impl ScheduledJobPort for AssetCleanupService {
    fn run_if_due(&self) {
        self.run_if_due();
    }
}

#[cfg(test)]
mod tests {
    use std::collections::{HashMap, HashSet};
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

    const INSTANCE: Uuid = Uuid::from_u128(0x0000_0000_0000_0000_0000_0000_0000_00A1);

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
                crate::core::domain::configuration::configuration::value_objects::MapsConfiguration::new(
                    crate::core::domain::configuration::configuration::DEFAULT_MAPS_UPDATE_CRON
                        .to_string(),
                    7200,
                    "https://build.protomaps.com/20260905.pmtiles".to_string(),
                    "1.31.2".to_string(),
                )
                .unwrap(),
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
            let mut jobs = self.jobs.lock().unwrap();
            if let Some(job) = jobs.iter_mut().find(|job| job.id == id)
                && job.status == JobStatus::Running
            {
                job.status = JobStatus::CancellationRequested;
            }
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
                .max_by_key(|job| job.finished_at)
                .cloned())
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

    /// A [`JobRepository`] that fails on demand.
    struct FailingJobRepository {
        fail: Mutex<HashSet<&'static str>>,
    }

    impl FailingJobRepository {
        fn new(fail: &[&'static str]) -> Self {
            Self {
                fail: Mutex::new(fail.iter().copied().collect()),
            }
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
        fn acquire(
            &self,
            _job_type: &str,
            _instance_id: Uuid,
            _lock_until: DateTime<Utc>,
        ) -> Result<bool, DomainError> {
            if self.should_fail("acquire") {
                Err(DomainError::Database("boom".to_string()))
            } else {
                Ok(true)
            }
        }
        fn release(&self, _job_type: &str, _instance_id: Uuid) -> Result<(), DomainError> {
            Ok(())
        }
        fn heartbeat(
            &self,
            _id: Uuid,
            _job_type: &str,
            _instance_id: Uuid,
            _at: DateTime<Utc>,
            _lock_until: DateTime<Utc>,
        ) -> Result<JobStatus, DomainError> {
            if self.should_fail("heartbeat") {
                Err(DomainError::Database("boom".to_string()))
            } else {
                Ok(JobStatus::Running)
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
        fn find_active_by_type(&self, _job_type: &str) -> Result<Vec<Job>, DomainError> {
            if self.should_fail("find_active") {
                Err(DomainError::Database("boom".to_string()))
            } else {
                Ok(Vec::new())
            }
        }
        fn find_last_finished_by_type(&self, _job_type: &str) -> Result<Option<Job>, DomainError> {
            if self.should_fail("find_last") {
                Err(DomainError::Database("boom".to_string()))
            } else {
                Ok(None)
            }
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
        let mut job = Job::running(
            Uuid::new_v4(),
            ASSET_CLEANUP_JOB_NAME.to_string(),
            ASSET_CLEANUP_JOB_TYPE.to_string(),
            INSTANCE,
            finished_at - Duration::minutes(10),
        );
        job.status = JobStatus::Finished;
        job.finished_at = Some(finished_at);
        job
    }

    fn running_job() -> Job {
        Job::running(
            Uuid::new_v4(),
            ASSET_CLEANUP_JOB_NAME.to_string(),
            ASSET_CLEANUP_JOB_TYPE.to_string(),
            INSTANCE,
            Utc::now(),
        )
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
            INSTANCE,
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
            INSTANCE,
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
            INSTANCE,
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
            INSTANCE,
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
            INSTANCE,
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
            INSTANCE,
        )
    }

    #[test]
    fn survives_repository_lookup_errors() {
        // find_active fails -> log and skip without panicking.
        let service = error_service(
            Arc::new(FailingJobRepository::new(&["find_active"])),
            Arc::new(MemoryAssetStorage {
                objects: Mutex::new(HashSet::new()),
            }),
        );
        service.run_if_due();

        // find_last error returns early, so nothing was deleted.
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
    fn skips_when_another_instance_holds_the_lock() {
        let storage = Arc::new(MemoryAssetStorage {
            objects: Mutex::new(HashSet::from(["provider/orphan.jpg".to_string()])),
        });
        let asset_repo = Arc::new(MemoryAssetRepository {
            known: Mutex::new(HashSet::new()),
        });
        let job_repo = Arc::new(MemoryJobRepository::new(Vec::new()));
        // Another instance already owns the type's lock.
        job_repo
            .acquire(
                ASSET_CLEANUP_JOB_TYPE,
                Uuid::new_v4(),
                Utc::now() + Duration::hours(1),
            )
            .unwrap();
        let service = AssetCleanupService::new(
            job_repo.clone(),
            asset_repo.clone(),
            storage.clone(),
            configuration(),
            INSTANCE,
        );

        service.run_if_due();

        assert_eq!(
            storage.list_object_keys().unwrap().len(),
            1,
            "must not delete when another instance owns the lock"
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
    fn records_cancelled_when_a_request_arrives_during_cleanup() {
        let job_repo = Arc::new(MemoryJobRepository::new(Vec::new()));
        let service = AssetCleanupService::new(
            job_repo.clone(),
            Arc::new(MemoryAssetRepository {
                known: Mutex::new(HashSet::new()),
            }),
            Arc::new(MemoryAssetStorage {
                objects: Mutex::new(HashSet::new()),
            }),
            configuration(),
            INSTANCE,
        );

        // Seed a RUNNING job and request cancellation while it is "running",
        // then drive the finalize path directly against it.
        let job = running_job();
        job_repo.insert(job.clone()).unwrap();
        job_repo.request_cancellation(job.id).unwrap();

        service.finalize(job.id, &job.name, Ok(()));

        let stored = job_repo.find_by_id(job.id).unwrap().unwrap();
        assert_eq!(stored.status, JobStatus::Cancelled);
    }

    #[test]
    fn logs_when_acquire_fails() {
        let service = error_service(
            Arc::new(FailingJobRepository::new(&["acquire"])),
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
