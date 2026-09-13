//! Application job that maintains the hourly/daily measurement rollups used by
//! the station analytics read model.
//!
//! Mirrors the other scheduled jobs: it is driven by the generic cron scheduler
//! through [`ScheduledJobPort`] and tracks itself as a `measurement_rollup` job.
//! On its first run (or after a wiped rollup) it backfills the whole history; on
//! later runs it refreshes only the recent window as a self-healing fallback.
//! The import flow additionally calls [`MeasurementRollupService::refresh`]
//! directly after each successful source import, so the aggregates are fresh
//! without waiting for the next cron tick.

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
use crate::core::domain::measurements::repository_port::MeasurementRepository;

/// The job type owned by this service.
pub const MEASUREMENT_ROLLUP_JOB_TYPE: &str = "measurement_rollup";
/// Human-readable name of the measurement rollup job.
pub const MEASUREMENT_ROLLUP_JOB_NAME: &str = "measurement rollup";

/// How many days back the periodic refresh re-aggregates. New data is usually
/// refreshed immediately after its import; this window is the self-healing
/// fallback for restarts and missed runs.
const ROLLUP_REFRESH_DAYS: i64 = 3;

/// The time span of one refresh transaction. The first-run backfill walks the
/// whole history in chunks of this size so each transaction stays small and the
/// job commits visible progress instead of doing one huge scan.
const ROLLUP_REFRESH_CHUNK_DAYS: i64 = 7;

pub struct MeasurementRollupService {
    job_repository: Arc<dyn JobRepository + Send + Sync>,
    measurement_repository: Arc<dyn MeasurementRepository + Send + Sync>,
    configuration: Arc<Configuration>,
    instance_id: Uuid,
}

impl MeasurementRollupService {
    pub fn new(
        job_repository: Arc<dyn JobRepository + Send + Sync>,
        measurement_repository: Arc<dyn MeasurementRepository + Send + Sync>,
        configuration: Arc<Configuration>,
        instance_id: Uuid,
    ) -> Self {
        Self {
            job_repository,
            measurement_repository,
            configuration,
            instance_id,
        }
    }

    /// Decides whether the rollup job should run now and executes it if so:
    /// never-succeeded runs a full backfill, an overdue last run refreshes the
    /// recent window. Skips while an active job exists.
    pub fn run_if_due(&self) {
        let now = Utc::now();

        if self.active_job_exists() {
            return;
        }

        match self
            .job_repository
            .find_last_finished_by_type(MEASUREMENT_ROLLUP_JOB_TYPE)
        {
            Ok(None) => {
                println!("Measurement rollup has never succeeded; running the full backfill");
                self.execute_full(now);
            }
            Ok(Some(last)) => {
                if self.is_overdue(&last, now) {
                    println!("Measurement rollup is overdue; refreshing the recent window");
                    self.execute_recent(now);
                }
            }
            Err(error) => {
                eprintln!("Failed to check the last finished measurement rollup job: {error:?}");
            }
        }
    }

    /// Refreshes the rollups for the given imported range. Called directly after
    /// an import so the aggregates are fresh without waiting for the cron tick.
    pub fn refresh(&self, from: DateTime<Utc>, to: DateTime<Utc>) -> Result<(), DomainError> {
        self.measurement_repository.refresh_rollups(from, to)
    }

    fn active_job_exists(&self) -> bool {
        match self
            .job_repository
            .find_active_by_type(MEASUREMENT_ROLLUP_JOB_TYPE)
        {
            Ok(active) if !active.is_empty() => {
                let ids = active
                    .iter()
                    .map(|job| job.id.to_string())
                    .collect::<Vec<_>>()
                    .join(", ");
                println!("Measurement rollup is still active ({ids}); skipping");
                true
            }
            Ok(_) => false,
            Err(error) => {
                eprintln!("Failed to check for an active measurement rollup job: {error:?}");
                true
            }
        }
    }

    /// Whether the last successful run is overdue: the next scheduled cron
    /// trigger after its finish time has already passed.
    fn is_overdue(&self, last: &Job, now: DateTime<Utc>) -> bool {
        let schedule = match cron::Schedule::from_str(self.configuration.measurement_rollup_cron())
        {
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

    fn execute_full(&self, now: DateTime<Utc>) {
        let bounds = match self.measurement_repository.measurement_bounds() {
            Ok(Some(bounds)) => bounds,
            Ok(None) => {
                println!("No measurements yet; nothing to roll up");
                return;
            }
            Err(error) => {
                eprintln!("Failed to read measurement bounds for the rollup backfill: {error:?}");
                return;
            }
        };
        // One minute of slack on each side so timestamps exactly at the bounds
        // are included by the half-open `[from, to)` refresh.
        let slack = chrono::Duration::minutes(1);
        self.execute(bounds.0 - slack, bounds.1 + slack, now);
    }

    fn execute_recent(&self, now: DateTime<Utc>) {
        let from = now - chrono::Duration::days(ROLLUP_REFRESH_DAYS);
        self.execute(from, now, now);
    }

    /// Runs one refresh as a tracked job owned by this instance.
    fn execute(&self, from: DateTime<Utc>, to: DateTime<Utc>, now: DateTime<Utc>) {
        let interval = self
            .configuration
            .measurement_rollup_max_heartbeat_interval();
        let instance_id = self.instance_id;

        match self
            .job_repository
            .acquire(MEASUREMENT_ROLLUP_JOB_TYPE, instance_id, now + interval)
        {
            Ok(true) => {}
            Ok(false) => {
                println!("Measurement rollup is already active elsewhere; skipping");
                return;
            }
            Err(error) => {
                eprintln!("Failed to acquire the measurement rollup lock: {error:?}");
                return;
            }
        }

        let job = Job::running(
            Uuid::new_v4(),
            MEASUREMENT_ROLLUP_JOB_NAME.to_string(),
            MEASUREMENT_ROLLUP_JOB_TYPE.to_string(),
            instance_id,
            now,
        );
        let job_id = job.id;
        let job_name = job.name.clone();
        if let Err(error) = self.job_repository.insert(job) {
            let _ = self
                .job_repository
                .release(MEASUREMENT_ROLLUP_JOB_TYPE, instance_id);
            eprintln!("Failed to record measurement rollup job {job_name} ({job_id}): {error:?}");
            return;
        }
        println!("Measurement rollup job {job_name} ({job_id}) started");

        let heartbeat = JobHeartbeat::start(
            self.job_repository.clone(),
            job_id,
            MEASUREMENT_ROLLUP_JOB_TYPE,
            instance_id,
            interval,
        );

        // Refresh in bounded chunks (each committing its own transaction) so a
        // long first-run backfill makes visible progress, keeps every
        // transaction small, and stays cancellable between chunks.
        let chunk = chrono::Duration::days(ROLLUP_REFRESH_CHUNK_DAYS);
        let mut cursor = from;
        let mut cancelled = self.is_cancelled_or_requested(job_id);
        let mut failure: Option<DomainError> = None;
        while !cancelled && cursor < to {
            let next = std::cmp::min(cursor + chunk, to);
            match self.measurement_repository.refresh_rollups(cursor, next) {
                Ok(()) => cursor = next,
                Err(error) => {
                    failure = Some(error);
                    break;
                }
            }
            cancelled = self.is_cancelled_or_requested(job_id);
        }
        heartbeat.stop();

        if let Some(error) = failure {
            let message = format!("{error:?}");
            if let Err(set_failed_error) =
                self.job_repository.set_failed(job_id, Utc::now(), &message)
            {
                eprintln!(
                    "Failed to mark measurement rollup job {job_name} ({job_id}) as failed: {set_failed_error:?}"
                );
            } else {
                eprintln!("Measurement rollup job {job_name} ({job_id}) failed: {error:?}");
            }
            let _ = self
                .job_repository
                .release(MEASUREMENT_ROLLUP_JOB_TYPE, self.instance_id);
            return;
        }
        if cancelled {
            self.finalize_cancelled(job_id, &job_name);
            return;
        }
        self.finalize_after_update(job_id, &job_name);
    }

    /// Heartbeats the job and returns whether a cancellation is already in
    /// flight (so the run stops instead of finishing).
    fn is_cancelled_or_requested(&self, job_id: Uuid) -> bool {
        let now = Utc::now();
        let interval = self
            .configuration
            .measurement_rollup_max_heartbeat_interval();
        match self.job_repository.heartbeat(
            job_id,
            MEASUREMENT_ROLLUP_JOB_TYPE,
            self.instance_id,
            now,
            now + interval,
        ) {
            Ok(JobStatus::CancellationRequested) | Ok(JobStatus::Cancelled) => true,
            Ok(_) => false,
            Err(error) => {
                eprintln!("Failed to heartbeat measurement rollup job {job_id}: {error:?}");
                false
            }
        }
    }

    /// Decides FINISHED vs CANCELLED after the refresh completed, then releases
    /// the type's lock.
    fn finalize_after_update(&self, job_id: Uuid, job_name: &str) {
        if self.is_cancelled_or_requested(job_id) {
            self.finalize_cancelled(job_id, job_name);
            return;
        }
        if let Err(error) = self.job_repository.set_finished(job_id, Utc::now()) {
            eprintln!("Failed to finish measurement rollup job {job_name} ({job_id}): {error:?}");
        } else {
            println!("Measurement rollup job {job_name} ({job_id}) finished");
        }
        let _ = self
            .job_repository
            .release(MEASUREMENT_ROLLUP_JOB_TYPE, self.instance_id);
    }

    /// Marks the job CANCELLED and releases the type's lock.
    fn finalize_cancelled(&self, job_id: Uuid, job_name: &str) {
        match self.job_repository.mark_cancelled(job_id, Utc::now()) {
            Ok(()) => println!("Measurement rollup job {job_name} ({job_id}) cancelled"),
            Err(error) => {
                eprintln!(
                    "Could not finalize measurement rollup job {job_name} ({job_id}) as cancelled: {error:?}"
                );
            }
        }
        let _ = self
            .job_repository
            .release(MEASUREMENT_ROLLUP_JOB_TYPE, self.instance_id);
    }
}

impl ScheduledJobPort for MeasurementRollupService {
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
    use crate::core::domain::measurements::measurement::{Measurement, value_objects};
    use crate::core::domain::measurements::repository_port::{
        BucketGranularity, ChannelBucket, ChannelHourTotal, ChannelTotal, HourTotal,
        MeasurementBounds, MeasurementRepository, MonthTotal, TimeBucket, WeekdayTotal,
    };

    const INSTANCE: Uuid = Uuid::from_u128(0x0000_0000_0000_0000_0000_0000_0000_00D2);

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

    struct MockMeasurementRepository {
        bounds: Option<(DateTime<Utc>, DateTime<Utc>)>,
        refreshes: Mutex<Vec<(DateTime<Utc>, DateTime<Utc>)>>,
        fail: bool,
        fail_bounds: bool,
    }

    impl MockMeasurementRepository {
        fn new(bounds: Option<(DateTime<Utc>, DateTime<Utc>)>, fail: bool) -> Self {
            Self {
                bounds,
                refreshes: Mutex::new(Vec::new()),
                fail,
                fail_bounds: false,
            }
        }

        /// A repository whose `measurement_bounds` probe fails.
        fn failing_bounds() -> Self {
            Self {
                bounds: None,
                refreshes: Mutex::new(Vec::new()),
                fail: false,
                fail_bounds: true,
            }
        }

        fn refreshes(&self) -> Vec<(DateTime<Utc>, DateTime<Utc>)> {
            self.refreshes.lock().unwrap().clone()
        }
    }

    impl MeasurementRepository for MockMeasurementRepository {
        fn save(&self, _measurement: Measurement) -> Result<(), DomainError> {
            unimplemented!()
        }

        fn save_batch(&self, _measurements: Vec<Measurement>) -> Result<u64, DomainError> {
            unimplemented!()
        }

        fn find_by_id(&self, _id: value_objects::Id) -> Result<Measurement, DomainError> {
            unimplemented!()
        }

        fn find_all(&self) -> Result<Vec<Measurement>, DomainError> {
            unimplemented!()
        }

        fn find_by_channel_id(
            &self,
            _channel_id: value_objects::ChannelId,
        ) -> Result<Vec<Measurement>, DomainError> {
            unimplemented!()
        }

        fn find_page(
            &self,
            _channel_id: Option<value_objects::ChannelId>,
            _offset: usize,
            _limit: usize,
        ) -> Result<Vec<Measurement>, DomainError> {
            unimplemented!()
        }

        fn sum(
            &self,
            _from: DateTime<Utc>,
            _to: DateTime<Utc>,
            _channel_ids: &[value_objects::ChannelId],
            _resolution_seconds: Option<i64>,
        ) -> Result<i64, DomainError> {
            unimplemented!()
        }

        fn sum_buckets(
            &self,
            _from: DateTime<Utc>,
            _to: DateTime<Utc>,
            _granularity: BucketGranularity,
            _origin: DateTime<Utc>,
            _timezone: &str,
            _channel_ids: &[value_objects::ChannelId],
            _resolution_seconds: Option<i64>,
        ) -> Result<Vec<TimeBucket>, DomainError> {
            unimplemented!()
        }

        fn sum_buckets_by_channel(
            &self,
            _from: DateTime<Utc>,
            _to: DateTime<Utc>,
            _granularity: BucketGranularity,
            _origin: DateTime<Utc>,
            _timezone: &str,
            _channel_ids: &[value_objects::ChannelId],
            _resolution_seconds: Option<i64>,
        ) -> Result<Vec<ChannelBucket>, DomainError> {
            unimplemented!()
        }

        fn sum_weekdays(
            &self,
            _from: DateTime<Utc>,
            _to: DateTime<Utc>,
            _timezone: &str,
            _channel_ids: &[value_objects::ChannelId],
            _resolution_seconds: Option<i64>,
        ) -> Result<Vec<WeekdayTotal>, DomainError> {
            unimplemented!()
        }

        fn sum_hours(
            &self,
            _from: DateTime<Utc>,
            _to: DateTime<Utc>,
            _timezone: &str,
            _channel_ids: &[value_objects::ChannelId],
            _resolution_seconds: Option<i64>,
        ) -> Result<Vec<HourTotal>, DomainError> {
            unimplemented!()
        }

        fn sum_hours_by_channel(
            &self,
            _from: DateTime<Utc>,
            _to: DateTime<Utc>,
            _timezone: &str,
            _channel_ids: &[value_objects::ChannelId],
            _resolution_seconds: Option<i64>,
        ) -> Result<Vec<ChannelHourTotal>, DomainError> {
            unimplemented!()
        }

        fn sum_by_channel(
            &self,
            _from: DateTime<Utc>,
            _to: DateTime<Utc>,
            _channel_ids: &[value_objects::ChannelId],
            _resolution_seconds: Option<i64>,
        ) -> Result<Vec<ChannelTotal>, DomainError> {
            unimplemented!()
        }

        fn sum_by_month(
            &self,
            _timezone: &str,
            _channel_ids: &[value_objects::ChannelId],
            _resolution_seconds: Option<i64>,
        ) -> Result<Vec<MonthTotal>, DomainError> {
            unimplemented!()
        }

        fn measurement_bounds(&self) -> Result<Option<MeasurementBounds>, DomainError> {
            if self.fail_bounds {
                return Err(DomainError::Database("bounds failed".to_string()));
            }
            Ok(self.bounds)
        }

        fn refresh_rollups(
            &self,
            from: DateTime<Utc>,
            to: DateTime<Utc>,
        ) -> Result<(), DomainError> {
            self.refreshes.lock().unwrap().push((from, to));
            if self.fail {
                Err(DomainError::Database("refresh failed".to_string()))
            } else {
                Ok(())
            }
        }
    }

    struct MemoryJobRepository {
        jobs: Mutex<Vec<Job>>,
        locks: Mutex<HashMap<String, (Uuid, DateTime<Utc>)>>,
        fail_find_active: bool,
        fail_find_last_finished: bool,
        fail_acquire: bool,
    }

    impl MemoryJobRepository {
        fn new(jobs: Vec<Job>) -> Self {
            Self {
                jobs: Mutex::new(jobs),
                locks: Mutex::new(HashMap::new()),
                fail_find_active: false,
                fail_find_last_finished: false,
                fail_acquire: false,
            }
        }

        fn with_failures(
            jobs: Vec<Job>,
            fail_find_active: bool,
            fail_find_last_finished: bool,
            fail_acquire: bool,
        ) -> Self {
            Self {
                jobs: Mutex::new(jobs),
                locks: Mutex::new(HashMap::new()),
                fail_find_active,
                fail_find_last_finished,
                fail_acquire,
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
            if self.fail_acquire {
                return Err(DomainError::Database("acquire failed".to_string()));
            }
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
            if self.fail_find_active {
                return Err(DomainError::Database("find_active failed".to_string()));
            }
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
            if self.fail_find_last_finished {
                return Err(DomainError::Database("find_last failed".to_string()));
            }
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
            MEASUREMENT_ROLLUP_JOB_NAME.to_string(),
            MEASUREMENT_ROLLUP_JOB_TYPE.to_string(),
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
            MEASUREMENT_ROLLUP_JOB_NAME.to_string(),
            MEASUREMENT_ROLLUP_JOB_TYPE.to_string(),
            INSTANCE,
            now,
        )
    }

    fn service(
        repo: Arc<MemoryJobRepository>,
        measurements: Arc<MockMeasurementRepository>,
    ) -> MeasurementRollupService {
        MeasurementRollupService::new(repo, measurements, configuration(), INSTANCE)
    }

    #[test]
    fn first_run_backfills_the_whole_history_in_chunks() {
        let from = Utc::now() - Duration::days(400);
        let to = Utc::now() - Duration::days(1);
        let repo = Arc::new(MemoryJobRepository::new(vec![]));
        let measurements = Arc::new(MockMeasurementRepository::new(Some((from, to)), false));

        service(repo.clone(), measurements.clone()).run_if_due();

        let slack = Duration::minutes(1);
        let refreshes = measurements.refreshes();
        assert!(refreshes.len() > 1, "the full backfill runs in chunks");
        assert_eq!(refreshes.first().unwrap().0, from - slack);
        assert_eq!(refreshes.last().unwrap().1, to + slack);
        for pair in refreshes.windows(2) {
            assert_eq!(pair[0].1, pair[1].0, "chunks must be contiguous");
            assert!(pair[1].1 - pair[1].0 <= Duration::days(ROLLUP_REFRESH_CHUNK_DAYS));
        }
        assert!(
            repo.all()
                .iter()
                .any(|job| job.status == JobStatus::Finished)
        );
    }

    #[test]
    fn overdue_run_refreshes_the_recent_window() {
        let repo = Arc::new(MemoryJobRepository::new(vec![finished_job(
            Utc::now() - Duration::days(200),
        )]));
        let measurements = Arc::new(MockMeasurementRepository::new(None, false));

        service(repo.clone(), measurements.clone()).run_if_due();

        let refreshes = measurements.refreshes();
        assert_eq!(refreshes.len(), 1);
        assert_eq!(
            refreshes[0].1 - refreshes[0].0,
            Duration::days(ROLLUP_REFRESH_DAYS)
        );
    }

    #[test]
    fn skips_while_an_active_job_exists() {
        let repo = Arc::new(MemoryJobRepository::new(vec![running_job(Utc::now())]));
        let measurements = Arc::new(MockMeasurementRepository::new(None, false));

        service(repo.clone(), measurements.clone()).run_if_due();

        assert!(measurements.refreshes().is_empty());
    }

    #[test]
    fn does_not_run_when_the_last_run_is_recent() {
        let repo = Arc::new(MemoryJobRepository::new(vec![finished_job(Utc::now())]));
        let measurements = Arc::new(MockMeasurementRepository::new(None, false));

        service(repo.clone(), measurements.clone()).run_if_due();

        assert!(measurements.refreshes().is_empty());
    }

    #[test]
    fn refresh_delegates_directly_without_job_tracking() {
        let repo = Arc::new(MemoryJobRepository::new(vec![]));
        let measurements = Arc::new(MockMeasurementRepository::new(None, false));
        let from = Utc::now() - Duration::days(2);
        let to = Utc::now();

        service(repo.clone(), measurements.clone())
            .refresh(from, to)
            .unwrap();

        assert_eq!(measurements.refreshes(), vec![(from, to)]);
    }

    #[test]
    fn records_failure_when_the_refresh_fails() {
        let repo = Arc::new(MemoryJobRepository::new(vec![]));
        let measurements = Arc::new(MockMeasurementRepository::new(
            Some((Utc::now() - Duration::days(10), Utc::now())),
            true,
        ));

        service(repo.clone(), measurements.clone()).run_if_due();

        assert!(
            repo.all().iter().any(|job| job.status == JobStatus::Failed),
            "expected a FAILED measurement_rollup job"
        );
    }

    #[test]
    fn skips_when_there_are_no_measurements() {
        let repo = Arc::new(MemoryJobRepository::new(vec![]));
        let measurements = Arc::new(MockMeasurementRepository::new(None, false));

        service(repo.clone(), measurements.clone()).run_if_due();

        assert!(measurements.refreshes().is_empty());
    }

    #[test]
    fn skips_when_the_bounds_lookup_fails() {
        let repo = Arc::new(MemoryJobRepository::new(vec![]));
        let measurements = Arc::new(MockMeasurementRepository::failing_bounds());

        service(repo.clone(), measurements.clone()).run_if_due();

        assert!(measurements.refreshes().is_empty());
    }

    #[test]
    fn skips_when_the_active_job_lookup_fails() {
        let repo = Arc::new(MemoryJobRepository::with_failures(
            vec![],
            true,
            false,
            false,
        ));
        let measurements = Arc::new(MockMeasurementRepository::new(None, false));

        service(repo.clone(), measurements.clone()).run_if_due();

        assert!(measurements.refreshes().is_empty());
    }

    #[test]
    fn skips_when_the_last_finished_lookup_fails() {
        let repo = Arc::new(MemoryJobRepository::with_failures(
            vec![],
            false,
            true,
            false,
        ));
        let measurements = Arc::new(MockMeasurementRepository::new(None, false));

        service(repo.clone(), measurements.clone()).run_if_due();

        assert!(measurements.refreshes().is_empty());
    }

    #[test]
    fn does_not_claim_when_another_instance_holds_the_lock() {
        let repo = Arc::new(MemoryJobRepository::new(vec![]));
        repo.acquire(
            MEASUREMENT_ROLLUP_JOB_TYPE,
            Uuid::new_v4(),
            Utc::now() + Duration::hours(1),
        )
        .unwrap();
        let measurements = Arc::new(MockMeasurementRepository::new(
            Some((Utc::now() - Duration::days(1), Utc::now())),
            false,
        ));

        service(repo.clone(), measurements.clone()).run_if_due();

        assert!(measurements.refreshes().is_empty());
    }

    #[test]
    fn skips_when_the_lock_cannot_be_acquired() {
        let repo = Arc::new(MemoryJobRepository::with_failures(
            vec![],
            false,
            false,
            true,
        ));
        let measurements = Arc::new(MockMeasurementRepository::new(
            Some((Utc::now() - Duration::days(1), Utc::now())),
            false,
        ));

        service(repo.clone(), measurements.clone()).run_if_due();

        assert!(measurements.refreshes().is_empty());
    }

    #[test]
    fn records_cancelled_when_a_request_arrives_during_the_refresh() {
        let repo = Arc::new(MemoryJobRepository::new(vec![]));
        let measurements = Arc::new(MockMeasurementRepository::new(None, false));
        let svc = service(repo.clone(), measurements.clone());

        let job = running_job(Utc::now());
        repo.insert(job.clone()).unwrap();
        repo.request_cancellation(job.id).unwrap();

        svc.finalize_after_update(job.id, &job.name);

        let stored = repo.find_by_id(job.id).unwrap().unwrap();
        assert_eq!(stored.status, JobStatus::Cancelled);
    }
}
