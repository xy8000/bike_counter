//! Application job runner that keeps the configured data sources up to date.
//!
//! Every run is tracked as a generic ShedLock-style job (see the `jobs` domain):
//! a PENDING job is inserted with a `lifetime_until` deadline, moved to RUNNING,
//! then to FINISHED (or FAILED). While a job is RUNNING and within its lifetime
//! it blocks other runs of the same type; stale RUNNING jobs past their
//! lifetime are expired (flagged with `max_lifetime_exceeded`).

use std::str::FromStr;
use std::sync::Arc;

use chrono::{DateTime, Utc};
use serde_json::json;
use uuid::Uuid;

use super::job_heartbeat::JobHeartbeat;
use crate::core::application::data_import_service::{
    DataImportService, DataSourceImportPhase, DataSourceRuntime,
};
use crate::core::domain::configuration::configuration::Configuration;
use crate::core::domain::data_source::data_source::value_objects::Id as DataSourceId;
use crate::core::domain::data_source::import_run::DataImportRun;
use crate::core::domain::data_source::import_run_port::DataImportRunRepository;
use crate::core::domain::data_source::repository_port::DataSourceRepository;
use crate::core::domain::data_source::service_port::DataSourceUpdateServicePort;
use crate::core::domain::error::DomainError;
use crate::core::domain::jobs::job::{Job, JobStatus};
use crate::core::domain::jobs::repository_port::JobRepository;
use crate::core::domain::jobs::scheduled_job_port::ScheduledJobPort;

/// The job type owned by this service.
pub const DATA_SOURCE_UPDATE_JOB_TYPE: &str = "data_source_update";
/// Human-readable name of the data-source update job.
pub const DATA_SOURCE_UPDATE_JOB_NAME: &str = "Data source update";

/// Job metadata key holding a source's running processed-measurement count (rows
/// read from the provider). Keys are grouped per data source by its UUID so the
/// counts of concurrently running sources never overwrite each other.
pub fn processed_measurements_key(data_source_id: DataSourceId) -> String {
    format!("{}_processed_measurements", data_source_id.0)
}

/// Job metadata key holding a source's running added-measurement count (rows
/// actually inserted), UUID-grouped like [`processed_measurements_key`].
pub fn added_measurements_key(data_source_id: DataSourceId) -> String {
    format!("{}_added_measurements", data_source_id.0)
}

/// Job metadata key holding the import phase a source is currently in (see
/// [`DataSourceImportPhase`]), UUID-grouped.
pub fn status_key(data_source_id: DataSourceId) -> String {
    format!("{}_status", data_source_id.0)
}

pub struct DataSourceUpdateService {
    job_repository: Arc<dyn JobRepository + Send + Sync>,
    data_source_repository: Arc<dyn DataSourceRepository + Send + Sync>,
    import_run_repository: Arc<dyn DataImportRunRepository + Send + Sync>,
    data_import_service: Arc<DataImportService>,
    configuration: Arc<Configuration>,
    runtimes: Vec<DataSourceRuntime>,
    instance_id: Uuid,
}

impl DataSourceUpdateService {
    pub fn new(
        job_repository: Arc<dyn JobRepository + Send + Sync>,
        data_source_repository: Arc<dyn DataSourceRepository + Send + Sync>,
        import_run_repository: Arc<dyn DataImportRunRepository + Send + Sync>,
        data_import_service: Arc<DataImportService>,
        configuration: Arc<Configuration>,
        runtimes: Vec<DataSourceRuntime>,
        instance_id: Uuid,
    ) -> Self {
        Self {
            job_repository,
            data_source_repository,
            import_run_repository,
            data_import_service,
            configuration,
            runtimes,
            instance_id,
        }
    }

    /// Decides whether the data-source update job should run now and executes
    /// it if so.
    ///
    /// The same always-on rule applies on every invocation (startup and cron
    /// ticks): run if the job has never succeeded or the last successful run is
    /// overdue (at least one scheduled cron trigger was missed since it
    /// finished). A RUNNING job within its lifetime always blocks: print a
    /// warning and do NOT start a second run. Stale RUNNING jobs past their
    /// `lifetime_until` are expired first, so the next job can proceed even
    /// after a worker crash.
    pub fn run_if_due(&self) {
        let now = Utc::now();

        // A RUNNING (or cancellation-requested) job blocks a new run; the
        // periodic watcher reconciles stale ones (see JobReconciliationService).
        match self.job_repository.find_active_by_type(DATA_SOURCE_UPDATE_JOB_TYPE) {
            Ok(active) if !active.is_empty() => {
                println!(
                    "Data source update job is still active ({} running/requesting); skipping",
                    active.len()
                );
                return;
            }
            Ok(_) => {}
            Err(error) => {
                eprintln!("Failed to check for an active data-source update job: {error:?}");
                return;
            }
        }

        // Run when the job has never succeeded or the last successful run is
        // overdue.
        match self
            .job_repository
            .find_last_finished_by_type(DATA_SOURCE_UPDATE_JOB_TYPE)
        {
            Ok(None) => {
                println!("Data source update job has never succeeded; running");
                self.execute(now);
            }
            Ok(Some(last)) => {
                if self.is_overdue(&last, now) {
                    println!(
                        "Data source update job is overdue (last run at {}); running",
                        last.finished_at
                            .map(|ts| ts.to_rfc3339())
                            .unwrap_or_else(|| "unknown".to_string())
                    );
                    self.execute(now);
                }
            }
            Err(error) => {
                eprintln!("Failed to check the last finished data-source update job: {error:?}");
            }
        }
    }

    /// Whether the last successful run is overdue: the next scheduled cron
    /// trigger after its finish time has already passed.
    fn is_overdue(&self, last: &Job, now: DateTime<Utc>) -> bool {
        let schedule = match cron::Schedule::from_str(self.configuration.data_source_update_cron())
        {
            Ok(schedule) => schedule,
            // The configuration validates the cron expression at construction;
            // this is a defensive fallback.
            Err(_) => return false,
        };
        match last.finished_at.or(last.started_at) {
            Some(anchor) => schedule
                .after(&anchor)
                .next()
                .is_some_and(|next| next <= now),
            // A finished job without timestamps: treat as overdue.
            None => true,
        }
    }

    /// Runs one full data-source update as a job owned by this instance.
    fn execute(&self, now: DateTime<Utc>) {
        let interval = self.configuration.data_source_update_max_heartbeat_interval();
        let instance_id = self.instance_id;

        // 1. Claim the type's lock; only the winning instance proceeds.
        match self
            .job_repository
            .acquire(DATA_SOURCE_UPDATE_JOB_TYPE, instance_id, now + interval)
        {
            Ok(true) => {}
            Ok(false) => {
                println!("Data source update is already running elsewhere; skipping");
                return;
            }
            Err(error) => {
                eprintln!("Failed to acquire the data source update lock: {error:?}");
                return;
            }
        }

        // 2. Record the RUNNING job owned by this instance.
        let job = Job::running(
            Uuid::new_v4(),
            DATA_SOURCE_UPDATE_JOB_NAME.to_string(),
            DATA_SOURCE_UPDATE_JOB_TYPE.to_string(),
            instance_id,
            now,
        );
        let job_id = job.id;
        let job_name = job.name.clone();
        if let Err(error) = self.job_repository.insert(job) {
            let _ = self.job_repository.release(DATA_SOURCE_UPDATE_JOB_TYPE, instance_id);
            eprintln!("Failed to record data-source update job {job_name} ({job_id}): {error:?}");
            return;
        }
        println!("Data source update job {job_name} ({job_id}) started");

        // 3. A dedicated heartbeat loop keeps the job fresh on a fixed tick,
        //    independent of import-batch boundaries (a slow provider or a large
        //    page must not let the watcher treat the job as stale).
        let heartbeat = JobHeartbeat::start(
            self.job_repository.clone(),
            job_id,
            DATA_SOURCE_UPDATE_JOB_TYPE,
            instance_id,
            interval,
        );
        let outcome = self.run_updates(job_id);
        heartbeat.stop();

        // 4. Finalize based on the resulting status.
        self.finalize(job_id, &job_name, outcome);
    }

    /// Finalizes the job according to `outcome` and the current persisted status,
    /// then always releases the type's lock.
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
                    Ok(()) => println!("Data source update job {job_name} ({job_id}) cancelled"),
                    Err(error) => eprintln!(
                        "Could not finalize data-source update job {job_name} ({job_id}) as cancelled: {error:?}"
                    ),
                }
            }
            (Ok(()), _) => {
                if let Err(error) = self.job_repository.set_finished(job_id, Utc::now()) {
                    eprintln!(
                        "Failed to finish data-source update job {job_name} ({job_id}): {error:?}"
                    );
                } else {
                    println!("Data source update job {job_name} ({job_id}) finished");
                }
            }
            (Err(error), _) => {
                let message = format!("{error:?}");
                if let Err(set_failed_error) =
                    self.job_repository.set_failed(job_id, Utc::now(), &message)
                {
                    eprintln!(
                        "Failed to mark data-source update job {job_name} ({job_id}) as failed: {set_failed_error:?}"
                    );
                } else {
                    eprintln!("Data source update job {job_name} ({job_id}) failed: {error:?}");
                }
            }
        }
        let _ = self.job_repository.release(DATA_SOURCE_UPDATE_JOB_TYPE, self.instance_id);
    }

    /// Updates **every configured data source in parallel**, one blocking thread
    /// per source, while keeping the aggregate job single. Each source runs
    /// through its source-level read ([`DataImportService::update_data_source`],
    /// which pages across the whole source via
    /// [`DataProvider::get_measurements_source`]): the run stops gracefully at
    /// the job `deadline` (`lifetime_until`) and the reported safe watermark is
    /// checkpointed into `imported_until` after every batch — so an interrupted
    /// (deadline/crash) run resumes instead of reprocessing.
    ///
    /// A deadline stop returns `Ok` (a partial but valid success): `execute`
    /// then marks the job FINISHED and the next scheduled run resumes from the
    /// checkpoint.
    ///
    /// All sources are started even when one fails; the returned error (which
    /// fails the aggregate job) summarizes every failing source.
    fn run_updates(&self, job_id: Uuid) -> Result<(), DomainError> {
        // This method is invoked from a blocking context (the scheduler wraps
        // `run_if_due` in `spawn_blocking`), so spawning plain OS threads is safe
        // — no tokio worker thread is ever blocked. `std::thread::scope` joins
        // every worker before returning.
        let failures = std::thread::scope(|scope| {
            let handles: Vec<_> = self
                .runtimes
                .iter()
                .map(|runtime| scope.spawn(move || self.update_one_source(job_id, runtime)))
                .collect();

            let mut failures = Vec::new();
            for handle in handles {
                match handle.join() {
                    Ok(Ok(())) => {}
                    Ok(Err(error)) => failures.push(error),
                    Err(_) => failures.push(DomainError::Provider(
                        "data-source import thread panicked".to_string(),
                    )),
                }
            }
            failures
        });

        if failures.is_empty() {
            Ok(())
        } else {
            let summary = failures
                .iter()
                .map(|error| format!("{error:?}"))
                .collect::<Vec<_>>()
                .join("; ");
            Err(DomainError::Provider(format!(
                "data source update failed ({} source(s)): {summary}",
                failures.len()
            )))
        }
    }

    /// Updates a single data source (one parallel worker): reads its
    /// `imported_until` anchor, records a per-source RUNNING [`DataImportRun`],
    /// pages through [`DataImportService::update_data_source_with_progress`] and
    /// finalizes the source (watermark/bounds/`last_updated_at`, import-run and
    /// per-source job-metadata status). Any error is returned so the aggregate
    /// job fails while the other sources still finish.
    fn update_one_source(
        &self,
        job_id: Uuid,
        runtime: &DataSourceRuntime,
    ) -> Result<(), DomainError> {
        let data_source_id = runtime.data_source_id;
        let from = self
            .data_source_repository
            .find_by_id(data_source_id)?
            .and_then(|data_source| data_source.imported_until);

        // Record a per-source RUNNING run so the UI can show this source's
        // last-import status/duration. Recording failures are non-fatal (the
        // import itself is the point; the run bookkeeping is best-effort).
        let started_at = Utc::now();
        let run_id = Uuid::new_v4();
        if let Err(error) = self.import_run_repository.insert(&DataImportRun::start(
            run_id,
            data_source_id,
            Some(job_id),
            started_at,
        )) {
            eprintln!("Failed to record import run {run_id}: {error:?}");
        }

        let result = self.data_import_service.update_data_source_with_progress(
            runtime,
            from,
            None,
            |processed, added, watermark| {
                self.job_repository.update_metadata(
                    job_id,
                    &processed_measurements_key(data_source_id),
                    json!(processed),
                )?;
                self.job_repository.update_metadata(
                    job_id,
                    &added_measurements_key(data_source_id),
                    json!(added),
                )?;
                // Checkpoint the safe watermark after every batch: an
                // interrupted run resumes from here.
                if let Some(watermark) = watermark {
                    self.data_source_repository
                        .update_imported_until(data_source_id, watermark)?;
                }
                // Heartbeat + cancellation check at the sub-task boundary (after
                // every batch) so a requested cancellation stops the run
                // gracefully with its watermark already checkpointed.
                self.check_cancellation(job_id)
            },
            |phase| {
                self.job_repository.update_metadata(
                    job_id,
                    &status_key(data_source_id),
                    json!(phase.as_str()),
                )
            },
        );

        match result {
            Ok(update) => {
                // Per-source success marker only when the whole source was
                // actually caught up.
                if update.completed {
                    self.data_source_repository
                        .update_last_updated(data_source_id, Utc::now())?;
                }
                // Persist the run's earliest/latest measurement timestamps as
                // the source's measurement bounds (the data-source detail page
                // reads these instead of scanning the history).
                if update.first_measurement_timestamp.is_some()
                    || update.last_measurement_timestamp_bound.is_some()
                {
                    self.data_source_repository.update_measurement_bounds(
                        data_source_id,
                        update.first_measurement_timestamp,
                        update.last_measurement_timestamp_bound,
                    )?;
                }
                // The import returned: record the final per-source status
                // (best-effort, like the import-run bookkeeping).
                if let Err(error) = self.job_repository.update_metadata(
                    job_id,
                    &status_key(data_source_id),
                    json!(DataSourceImportPhase::Finished.as_str()),
                ) {
                    eprintln!("Failed to record FINISHED status for {data_source_id:?}: {error:?}");
                }
                if let Err(error) = self.import_run_repository.finish(run_id, Utc::now()) {
                    eprintln!("Failed to finish import run {run_id}: {error:?}");
                }
                Ok(())
            }
            Err(DomainError::Cancelled) => {
                // Graceful cooperative stop: the watermark is already
                // checkpointed. Record the per-source phase as finished and
                // finish the import run so the source does not appear stuck;
                // the aggregate job itself is finalized as CANCELLED by
                // `execute`/`finalize`.
                if let Err(status_error) = self.job_repository.update_metadata(
                    job_id,
                    &status_key(data_source_id),
                    json!(DataSourceImportPhase::Finished.as_str()),
                ) {
                    eprintln!(
                        "Failed to record FINISHED status for {data_source_id:?}: {status_error:?}"
                    );
                }
                if let Err(run_error) = self.import_run_repository.finish(run_id, Utc::now()) {
                    eprintln!("Failed to finish import run {run_id}: {run_error:?}");
                }
                Ok(())
            }
            Err(error) => {
                if let Err(run_error) =
                    self.import_run_repository
                        .fail(run_id, Utc::now(), &format!("{error:?}"))
                {
                    eprintln!("Failed to fail import run {run_id}: {run_error:?}");
                }
                if let Err(status_error) = self.job_repository.update_metadata(
                    job_id,
                    &status_key(data_source_id),
                    json!(DataSourceImportPhase::Failed.as_str()),
                ) {
                    eprintln!(
                        "Failed to record FAILED status for {data_source_id:?}: {status_error:?}"
                    );
                }
                Err(error)
            }
        }
    }

    /// Heartbeats the job and returns [`DomainError::Cancelled`] when a
    /// cancellation was requested, so the batch loop stops gracefully.
    fn check_cancellation(&self, job_id: Uuid) -> Result<(), DomainError> {
        let now = Utc::now();
        let interval = self.configuration.data_source_update_max_heartbeat_interval();
        match self.job_repository.heartbeat(
            job_id,
            DATA_SOURCE_UPDATE_JOB_TYPE,
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

impl DataSourceUpdateServicePort for DataSourceUpdateService {
    fn run_if_due(&self) {
        self.run_if_due();
    }
}

impl ScheduledJobPort for DataSourceUpdateService {
    fn run_if_due(&self) {
        self.run_if_due();
    }
}

#[cfg(test)]
mod tests {
    use std::collections::{HashMap, VecDeque};
    use std::sync::{Arc, Mutex};

    use chrono::{DateTime, Duration, Utc};

    use super::*;
    use crate::core::domain::channels::channel::Channel;
    use crate::core::domain::channels::channel::value_objects as channel_vo;
    use crate::core::domain::channels::repository_port::ChannelRepository;
    use crate::core::domain::configuration::configuration::value_objects::{
        AssetStorageConfiguration, DataProviderConfiguration, DataSourceConfiguration,
        DatabaseConfiguration,
    };
    use crate::core::domain::configuration::configuration::{
        Configuration, DEFAULT_ASSET_CLEANUP_CRON, DEFAULT_DATA_SOURCE_UPDATE_CRON,
    };
    use crate::core::domain::counting_stations::counting_station::CountingStation;
    use crate::core::domain::counting_stations::counting_station::value_objects as station_vo;
    use crate::core::domain::counting_stations::repository_port::CountingStationRepository;
    use crate::core::domain::data_source::data_source::DataSource;
    use crate::core::domain::data_source::data_source::value_objects::Id as DataSourceId;
    use crate::core::domain::data_source::import_run::DataImportRun;
    use crate::core::domain::data_source::import_run_port::DataImportRunRepository;
    use crate::core::domain::data_source::provider_port::{
        ChannelRecord, CountingStationRecord, DataProvider, MeasurementRecord, ProviderError,
        SourceMeasurement, SourceMeasurementBatch,
    };
    use crate::core::domain::data_source::repository_port::DataSourceRepository;
    use crate::core::domain::health::HealthStatus;
    use crate::core::domain::jobs::job::JobStatus;
    use crate::core::domain::measurements::measurement::Measurement;
    use crate::core::domain::measurements::measurement::value_objects as measurement_vo;
    use crate::core::domain::measurements::repository_port::MeasurementRepository;

    fn asset_storage() -> AssetStorageConfiguration {
        AssetStorageConfiguration::new(
            "http://minio:9000".to_string(),
            "minioadmin".to_string(),
            "minioadmin".to_string(),
            "bike-counter-images".to_string(),
            "us-east-1".to_string(),
        )
        .unwrap()
    }

    fn configuration() -> Configuration {
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
            asset_storage(),
            DEFAULT_ASSET_CLEANUP_CRON.to_string(),
            3600,
            crate::core::domain::configuration::configuration::value_objects::MapsConfiguration::new(
                crate::core::domain::configuration::configuration::DEFAULT_MAPS_UPDATE_CRON
                    .to_string(),
                7200,
                "https://build.protomaps.com/20260829.pmtiles".to_string(),
                "1.31.2".to_string(),
            )
            .unwrap(),
        )
        .unwrap()
    }

    fn data_source_config() -> DataSourceConfiguration {
        DataSourceConfiguration::new(
            "Münster".to_string(),
            DataProviderConfiguration::new("provider".to_string(), HashMap::new()).unwrap(),
        )
        .unwrap()
    }

    fn station_record(external_id: &str) -> CountingStationRecord {
        CountingStationRecord {
            external_id: external_id.to_string(),
            name: format!("Station {external_id}"),
            description: "desc".to_string(),
            latitude: None,
            longitude: None,
            timezone: "Europe/Berlin".to_string(),
            image_sha256: None,
        }
    }

    fn channel_record(external_id: &str, station_external_id: &str) -> ChannelRecord {
        ChannelRecord {
            external_id: external_id.to_string(),
            counting_station_external_id: station_external_id.to_string(),
            name: format!("Channel {external_id}"),
            description: "desc".to_string(),
        }
    }

    fn measurement_record(value: i64, timestamp: DateTime<Utc>) -> MeasurementRecord {
        MeasurementRecord {
            value,
            timestamp,
            resolution_seconds: 3600,
            interval_end: None,
        }
    }

    /// An in-memory job repository that records the full lifecycle.
    struct MockJobRepository {
        jobs: Mutex<Vec<Job>>,
        fail_set_running: bool,
        fail_expire: bool,
        fail_insert: bool,
        fail_find_running: bool,
        fail_find_last_finished: bool,
        fail_set_finished: bool,
        fail_set_failed: bool,
    }

    impl MockJobRepository {
        fn new(jobs: Vec<Job>) -> Self {
            Self {
                jobs: Mutex::new(jobs),
                fail_set_running: false,
                fail_expire: false,
                fail_insert: false,
                fail_find_running: false,
                fail_find_last_finished: false,
                fail_set_finished: false,
                fail_set_failed: false,
            }
        }

        fn set_fail_set_running(&mut self, fail: bool) {
            self.fail_set_running = fail;
        }

        fn set_fail_expire(&mut self, fail: bool) {
            self.fail_expire = fail;
        }

        fn set_fail_insert(&mut self, fail: bool) {
            self.fail_insert = fail;
        }

        fn set_fail_find_running(&mut self, fail: bool) {
            self.fail_find_running = fail;
        }

        fn set_fail_find_last_finished(&mut self, fail: bool) {
            self.fail_find_last_finished = fail;
        }

        fn set_fail_set_finished(&mut self, fail: bool) {
            self.fail_set_finished = fail;
        }

        fn set_fail_set_failed(&mut self, fail: bool) {
            self.fail_set_failed = fail;
        }

        fn jobs(&self) -> Vec<Job> {
            self.jobs.lock().unwrap().clone()
        }
    }

    impl JobRepository for MockJobRepository {
        fn insert(&self, job: Job) -> Result<(), DomainError> {
            if self.fail_insert {
                return Err(DomainError::Database("insert failed".to_string()));
            }
            if job.lifetime_until <= Utc::now() {
                return Err(DomainError::InvalidQuery(
                    "a job requires a lifetime_until deadline in the future".to_string(),
                ));
            }
            self.jobs.lock().unwrap().push(job);
            Ok(())
        }

        fn set_running(&self, id: Uuid, started_at: DateTime<Utc>) -> Result<(), DomainError> {
            if self.fail_set_running {
                return Err(DomainError::InvalidQuery("cannot start".to_string()));
            }
            let mut jobs = self.jobs.lock().unwrap();
            let job = jobs
                .iter_mut()
                .find(|job| job.id == id)
                .ok_or(DomainError::NotFound(id))?;
            if job.status != JobStatus::Pending {
                return Err(DomainError::InvalidQuery(format!(
                    "job {id} is not in PENDING state"
                )));
            }
            job.status = JobStatus::Running;
            job.started_at = Some(started_at);
            Ok(())
        }

        fn set_finished(&self, id: Uuid, finished_at: DateTime<Utc>) -> Result<(), DomainError> {
            if self.fail_set_finished {
                return Err(DomainError::Database("set_finished failed".to_string()));
            }
            let mut jobs = self.jobs.lock().unwrap();
            let job = jobs
                .iter_mut()
                .find(|job| job.id == id)
                .ok_or(DomainError::NotFound(id))?;
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
            if self.fail_set_failed {
                return Err(DomainError::Database("set_failed failed".to_string()));
            }
            let mut jobs = self.jobs.lock().unwrap();
            let job = jobs
                .iter_mut()
                .find(|job| job.id == id)
                .ok_or(DomainError::NotFound(id))?;
            job.status = JobStatus::Failed;
            job.finished_at = Some(finished_at);
            job.failure_message = Some(message.to_string());
            Ok(())
        }

        fn update_metadata(
            &self,
            id: Uuid,
            key: &str,
            value: serde_json::Value,
        ) -> Result<(), DomainError> {
            let mut jobs = self.jobs.lock().unwrap();
            let job = jobs
                .iter_mut()
                .find(|job| job.id == id)
                .ok_or(DomainError::NotFound(id))?;
            job.metadata.insert(key.to_string(), value);
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
            if self.fail_find_running {
                return Err(DomainError::Database("find running failed".to_string()));
            }
            Ok(self
                .jobs
                .lock()
                .unwrap()
                .iter()
                .find(|job| job.job_type == job_type && job.status == JobStatus::Running)
                .cloned())
        }

        fn find_last_finished_by_type(&self, job_type: &str) -> Result<Option<Job>, DomainError> {
            if self.fail_find_last_finished {
                return Err(DomainError::Database("find last failed".to_string()));
            }
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
            job_type: &str,
            now: DateTime<Utc>,
        ) -> Result<u64, DomainError> {
            if self.fail_expire {
                return Err(DomainError::Database("expire failed".to_string()));
            }
            let mut jobs = self.jobs.lock().unwrap();
            let mut expired = 0u64;
            for job in jobs.iter_mut() {
                if job.job_type == job_type
                    && job.status == JobStatus::Running
                    && now > job.lifetime_until
                {
                    job.status = JobStatus::Failed;
                    job.finished_at = Some(now);
                    job.failure_message = Some("Max lifetime exceeded".to_string());
                    job.max_lifetime_exceeded = true;
                    expired += 1;
                }
            }
            Ok(expired)
        }
    }

    /// In-memory data-source repository; `fail_find` simulates DB errors.
    struct MockDataSourceRepository {
        data_sources: Mutex<Vec<DataSource>>,
        fail_find: bool,
        fail_update_imported_until: bool,
        fail_update_last_updated: bool,
        fail_update_measurement_bounds: bool,
    }

    impl MockDataSourceRepository {
        fn new(data_sources: Vec<DataSource>) -> Self {
            Self {
                data_sources: Mutex::new(data_sources),
                fail_find: false,
                fail_update_imported_until: false,
                fail_update_last_updated: false,
                fail_update_measurement_bounds: false,
            }
        }

        fn set_fail_find(&mut self, fail: bool) {
            self.fail_find = fail;
        }

        fn set_fail_update_imported_until(&mut self, fail: bool) {
            self.fail_update_imported_until = fail;
        }

        fn set_fail_update_last_updated(&mut self, fail: bool) {
            self.fail_update_last_updated = fail;
        }

        fn set_fail_update_measurement_bounds(&mut self, fail: bool) {
            self.fail_update_measurement_bounds = fail;
        }
    }

    impl DataSourceRepository for MockDataSourceRepository {
        fn upsert(&self, _data_source: DataSource) -> Result<(), DomainError> {
            Ok(())
        }

        fn find_by_id(&self, id: DataSourceId) -> Result<Option<DataSource>, DomainError> {
            if self.fail_find {
                return Err(DomainError::Database("find failed".to_string()));
            }
            Ok(self
                .data_sources
                .lock()
                .unwrap()
                .iter()
                .find(|ds| ds.id == id)
                .cloned())
        }

        fn find_by_name(&self, _name: &str) -> Result<Option<DataSource>, DomainError> {
            Ok(None)
        }

        fn find_all(&self) -> Result<Vec<DataSource>, DomainError> {
            Ok(self.data_sources.lock().unwrap().clone())
        }

        fn delete(&self, _id: DataSourceId) -> Result<(), DomainError> {
            Ok(())
        }

        fn update_imported_until(
            &self,
            id: DataSourceId,
            timestamp: DateTime<Utc>,
        ) -> Result<(), DomainError> {
            if self.fail_update_imported_until {
                return Err(DomainError::Database(
                    "update imported_until failed".to_string(),
                ));
            }
            if let Some(ds) = self
                .data_sources
                .lock()
                .unwrap()
                .iter_mut()
                .find(|ds| ds.id == id)
            {
                ds.imported_until = Some(timestamp);
            }
            Ok(())
        }

        fn clear_imported_until(&self, id: DataSourceId) -> Result<(), DomainError> {
            if let Some(ds) = self
                .data_sources
                .lock()
                .unwrap()
                .iter_mut()
                .find(|ds| ds.id == id)
            {
                ds.imported_until = None;
            }
            Ok(())
        }

        fn update_last_updated(
            &self,
            id: DataSourceId,
            timestamp: DateTime<Utc>,
        ) -> Result<(), DomainError> {
            if self.fail_update_last_updated {
                return Err(DomainError::Database(
                    "update last_updated failed".to_string(),
                ));
            }
            if let Some(ds) = self
                .data_sources
                .lock()
                .unwrap()
                .iter_mut()
                .find(|ds| ds.id == id)
            {
                ds.last_updated_at = Some(timestamp);
            }
            Ok(())
        }

        fn update_measurement_bounds(
            &self,
            id: DataSourceId,
            first: Option<DateTime<Utc>>,
            last: Option<DateTime<Utc>>,
        ) -> Result<(), DomainError> {
            if self.fail_update_measurement_bounds {
                return Err(DomainError::Database(
                    "update measurement bounds failed".to_string(),
                ));
            }
            if let Some(ds) = self
                .data_sources
                .lock()
                .unwrap()
                .iter_mut()
                .find(|ds| ds.id == id)
            {
                if let Some(first) = first {
                    ds.first_measurement_at = Some(
                        ds.first_measurement_at
                            .map_or(first, |existing| existing.min(first)),
                    );
                }
                if let Some(last) = last {
                    ds.last_measurement_at = Some(
                        ds.last_measurement_at
                            .map_or(last, |existing| existing.max(last)),
                    );
                }
            }
            Ok(())
        }
    }

    /// Records station/channel/measurement saves (never actually queried here).
    struct RecordingDataRepositories {
        stations: Mutex<Vec<CountingStation>>,
        channels: Mutex<Vec<Channel>>,
        measurements: Mutex<Vec<Measurement>>,
    }

    impl RecordingDataRepositories {
        fn new() -> Self {
            Self {
                stations: Mutex::new(Vec::new()),
                channels: Mutex::new(Vec::new()),
                measurements: Mutex::new(Vec::new()),
            }
        }
    }

    impl CountingStationRepository for RecordingDataRepositories {
        fn save(&self, station: CountingStation) -> Result<(), DomainError> {
            self.stations.lock().unwrap().push(station);
            Ok(())
        }

        fn update(&self, station: CountingStation) -> Result<(), DomainError> {
            let mut stations = self.stations.lock().unwrap();
            if let Some(existing) = stations.iter_mut().find(|s| s.id == station.id) {
                *existing = station;
            }
            Ok(())
        }

        fn find_by_id(&self, _id: station_vo::Id) -> Result<CountingStation, DomainError> {
            Err(DomainError::NotFound(Uuid::new_v4()))
        }

        fn find_all(&self) -> Result<Vec<CountingStation>, DomainError> {
            Ok(self.stations.lock().unwrap().clone())
        }

        fn find_by_external_datasource_id(
            &self,
            _external_id: station_vo::ExternalDatasourceId,
        ) -> Result<Option<CountingStation>, DomainError> {
            Ok(None)
        }

        fn find_filtered(&self, _name: Option<&str>) -> Result<Vec<CountingStation>, DomainError> {
            Ok(Vec::new())
        }
    }

    impl ChannelRepository for RecordingDataRepositories {
        fn save(&self, channel: Channel) -> Result<(), DomainError> {
            self.channels.lock().unwrap().push(channel);
            Ok(())
        }

        fn find_by_id(&self, _id: channel_vo::Id) -> Result<Channel, DomainError> {
            Err(DomainError::NotFound(Uuid::new_v4()))
        }

        fn find_all(&self) -> Result<Vec<Channel>, DomainError> {
            Ok(self.channels.lock().unwrap().clone())
        }

        fn find_by_counting_station_id(
            &self,
            _station_id: channel_vo::CountingStationId,
        ) -> Result<Vec<Channel>, DomainError> {
            Ok(Vec::new())
        }

        fn find_by_external_datasource_id(
            &self,
            _external_id: channel_vo::ExternalDatasourceId,
        ) -> Result<Option<Channel>, DomainError> {
            Ok(None)
        }

        fn find_filtered(
            &self,
            _counting_station_id: Option<channel_vo::CountingStationId>,
            _name: Option<&str>,
        ) -> Result<Vec<Channel>, DomainError> {
            Ok(Vec::new())
        }
    }

    impl MeasurementRepository for RecordingDataRepositories {
        fn save(&self, measurement: Measurement) -> Result<(), DomainError> {
            self.measurements.lock().unwrap().push(measurement);
            Ok(())
        }

        fn save_batch(&self, measurements: Vec<Measurement>) -> Result<u64, DomainError> {
            let len = measurements.len() as u64;
            self.measurements.lock().unwrap().extend(measurements);
            Ok(len)
        }

        fn find_by_id(&self, _id: measurement_vo::Id) -> Result<Measurement, DomainError> {
            Err(DomainError::NotFound(Uuid::new_v4()))
        }

        fn find_all(&self) -> Result<Vec<Measurement>, DomainError> {
            Ok(self.measurements.lock().unwrap().clone())
        }

        fn find_by_channel_id(
            &self,
            _channel_id: measurement_vo::ChannelId,
        ) -> Result<Vec<Measurement>, DomainError> {
            Ok(Vec::new())
        }

        fn find_page(
            &self,
            _channel_id: Option<measurement_vo::ChannelId>,
            _offset: usize,
            _limit: usize,
        ) -> Result<Vec<Measurement>, DomainError> {
            Ok(Vec::new())
        }

        fn sum(
            &self,
            _from: chrono::DateTime<chrono::Utc>,
            _to: chrono::DateTime<chrono::Utc>,
            _channel_ids: &[measurement_vo::ChannelId],
            _resolution_seconds: Option<i64>,
        ) -> Result<i64, DomainError> {
            Ok(0)
        }

        fn sum_buckets(
            &self,
            _from: chrono::DateTime<chrono::Utc>,
            _to: chrono::DateTime<chrono::Utc>,
            _granularity: crate::core::domain::measurements::repository_port::BucketGranularity,
            _origin: chrono::DateTime<chrono::Utc>,
            _timezone: &str,
            _channel_ids: &[measurement_vo::ChannelId],
            _resolution_seconds: Option<i64>,
        ) -> Result<Vec<crate::core::domain::measurements::repository_port::TimeBucket>, DomainError>
        {
            Ok(Vec::new())
        }

        fn sum_buckets_by_channel(
            &self,
            _from: chrono::DateTime<chrono::Utc>,
            _to: chrono::DateTime<chrono::Utc>,
            _granularity: crate::core::domain::measurements::repository_port::BucketGranularity,
            _origin: chrono::DateTime<chrono::Utc>,
            _timezone: &str,
            _channel_ids: &[measurement_vo::ChannelId],
            _resolution_seconds: Option<i64>,
        ) -> Result<
            Vec<crate::core::domain::measurements::repository_port::ChannelBucket>,
            DomainError,
        > {
            Ok(Vec::new())
        }

        fn sum_weekdays(
            &self,
            _from: chrono::DateTime<chrono::Utc>,
            _to: chrono::DateTime<chrono::Utc>,
            _timezone: &str,
            _channel_ids: &[measurement_vo::ChannelId],
            _resolution_seconds: Option<i64>,
        ) -> Result<
            Vec<crate::core::domain::measurements::repository_port::WeekdayTotal>,
            DomainError,
        > {
            Ok(Vec::new())
        }

        fn sum_hours(
            &self,
            _from: chrono::DateTime<chrono::Utc>,
            _to: chrono::DateTime<chrono::Utc>,
            _timezone: &str,
            _channel_ids: &[measurement_vo::ChannelId],
            _resolution_seconds: Option<i64>,
        ) -> Result<Vec<crate::core::domain::measurements::repository_port::HourTotal>, DomainError>
        {
            Ok(Vec::new())
        }

        fn sum_hours_by_channel(
            &self,
            _from: chrono::DateTime<chrono::Utc>,
            _to: chrono::DateTime<chrono::Utc>,
            _timezone: &str,
            _channel_ids: &[measurement_vo::ChannelId],
            _resolution_seconds: Option<i64>,
        ) -> Result<
            Vec<crate::core::domain::measurements::repository_port::ChannelHourTotal>,
            DomainError,
        > {
            Ok(Vec::new())
        }
        fn sum_by_channel(
            &self,
            _from: chrono::DateTime<chrono::Utc>,
            _to: chrono::DateTime<chrono::Utc>,
            _channel_ids: &[measurement_vo::ChannelId],
            _resolution_seconds: Option<i64>,
        ) -> Result<
            Vec<crate::core::domain::measurements::repository_port::ChannelTotal>,
            DomainError,
        > {
            Ok(Vec::new())
        }

        fn sum_by_month(
            &self,
            _timezone: &str,
            _channel_ids: &[measurement_vo::ChannelId],
            _resolution_seconds: Option<i64>,
        ) -> Result<Vec<crate::core::domain::measurements::repository_port::MonthTotal>, DomainError>
        {
            Ok(Vec::new())
        }
    }

    /// A provider that serves fixed external-id records and source-level pages.
    struct ScriptedProvider {
        stations: Vec<CountingStationRecord>,
        channels: Vec<ChannelRecord>,
        batches: Mutex<VecDeque<SourceMeasurementBatch>>,
    }

    impl DataProvider for ScriptedProvider {
        fn check_health(&self) -> HealthStatus {
            HealthStatus::Up
        }

        fn get_all_counting_stations(&self) -> Result<Vec<CountingStationRecord>, ProviderError> {
            Ok(self.stations.clone())
        }

        fn get_all_channels(&self) -> Result<Vec<ChannelRecord>, ProviderError> {
            Ok(self.channels.clone())
        }

        fn get_measurements_source(
            &self,
            _from: Option<DateTime<Utc>>,
            _max_batch_size: usize,
        ) -> Result<SourceMeasurementBatch, ProviderError> {
            self.batches
                .lock()
                .unwrap()
                .pop_front()
                .ok_or_else(|| ProviderError::InvalidData("no more pages".to_string()))
        }

        fn max_measurement_batch_size(&self) -> usize {
            100
        }
    }

    fn runtime(provider: Arc<dyn DataProvider>) -> DataSourceRuntime {
        DataSourceRuntime {
            configuration: data_source_config(),
            data_source_id: DataSourceId(DataSource::id_from_name("Münster")),
            provider,
        }
    }

    fn running_job(job_type: &str, started: DateTime<Utc>, lifetime_until: DateTime<Utc>) -> Job {
        let mut job = Job::new(
            Uuid::new_v4(),
            "Data source update".to_string(),
            job_type.to_string(),
            lifetime_until,
        );
        job.status = JobStatus::Running;
        job.started_at = Some(started);
        job
    }

    fn finished_job(job_type: &str) -> Job {
        let now = Utc::now();
        let mut job = Job::new(
            Uuid::new_v4(),
            "Data source update".to_string(),
            job_type.to_string(),
            now + Duration::seconds(3600),
        );
        job.status = JobStatus::Finished;
        job.started_at = Some(now - Duration::seconds(60));
        job.finished_at = Some(now);
        job
    }

    /// A FINISHED job whose `finished_at` is the given instant, so tests can
    /// control whether the last successful run is overdue.
    fn finished_job_at(job_type: &str, finished_at: DateTime<Utc>) -> Job {
        let mut job = Job::new(
            Uuid::new_v4(),
            "Data source update".to_string(),
            job_type.to_string(),
            finished_at + Duration::seconds(3600),
        );
        job.status = JobStatus::Finished;
        job.started_at = Some(finished_at - Duration::seconds(60));
        job.finished_at = Some(finished_at);
        job
    }

    /// An in-memory import-run repository recording the runs written by the
    /// service under test (the run bookkeeping must not disturb the job-level
    /// assertions the other tests make).
    #[derive(Default)]
    struct MemoryImportRunRepository {
        runs: Mutex<Vec<DataImportRun>>,
        fail_insert: Mutex<bool>,
        fail_finish: Mutex<bool>,
        fail_fail: Mutex<bool>,
    }

    impl MemoryImportRunRepository {
        fn set_fail_insert(&self, fail: bool) {
            *self.fail_insert.lock().unwrap() = fail;
        }

        fn set_fail_finish(&self, fail: bool) {
            *self.fail_finish.lock().unwrap() = fail;
        }

        fn set_fail_fail(&self, fail: bool) {
            *self.fail_fail.lock().unwrap() = fail;
        }
    }

    impl DataImportRunRepository for MemoryImportRunRepository {
        fn insert(&self, run: &DataImportRun) -> Result<(), DomainError> {
            if *self.fail_insert.lock().unwrap() {
                return Err(DomainError::Database("insert run failed".to_string()));
            }
            self.runs.lock().unwrap().push(run.clone());
            Ok(())
        }

        fn finish(&self, _id: Uuid, _finished_at: DateTime<Utc>) -> Result<(), DomainError> {
            if *self.fail_finish.lock().unwrap() {
                return Err(DomainError::Database("finish run failed".to_string()));
            }
            Ok(())
        }

        fn fail(
            &self,
            _id: Uuid,
            _finished_at: DateTime<Utc>,
            _message: &str,
        ) -> Result<(), DomainError> {
            if *self.fail_fail.lock().unwrap() {
                return Err(DomainError::Database("fail run failed".to_string()));
            }
            Ok(())
        }

        fn latest_by_data_source(
            &self,
            data_source_id: DataSourceId,
        ) -> Result<Option<DataImportRun>, DomainError> {
            Ok(self
                .runs
                .lock()
                .unwrap()
                .iter()
                .filter(|run| run.data_source_id == data_source_id)
                .max_by_key(|run| run.started_at)
                .cloned())
        }
    }

    /// A service wired to the given (fault-injectable) import-run repository.
    fn service_with_import_runs(
        job_repo: Arc<MockJobRepository>,
        data_source_repo: Arc<MockDataSourceRepository>,
        import_runs: Arc<MemoryImportRunRepository>,
        runtimes: Vec<DataSourceRuntime>,
    ) -> DataSourceUpdateService {
        let data_repos = Arc::new(RecordingDataRepositories::new());
        let station_repo: Arc<dyn CountingStationRepository + Send + Sync> = data_repos.clone();
        let channel_repo: Arc<dyn ChannelRepository + Send + Sync> = data_repos.clone();
        let measurement_repo: Arc<dyn MeasurementRepository + Send + Sync> = data_repos.clone();
        let data_import = Arc::new(DataImportService::new(
            station_repo,
            channel_repo,
            measurement_repo,
            Vec::new(),
        ));
        DataSourceUpdateService::new(
            job_repo,
            data_source_repo,
            import_runs,
            data_import,
            Arc::new(configuration()),
            runtimes,
        )
    }

    /// A service with a default (never-failing) import-run repository.
    fn service_with(
        job_repo: Arc<MockJobRepository>,
        data_source_repo: Arc<MockDataSourceRepository>,
        runtimes: Vec<DataSourceRuntime>,
    ) -> DataSourceUpdateService {
        service_with_import_runs(
            job_repo,
            data_source_repo,
            Arc::new(MemoryImportRunRepository::default()),
            runtimes,
        )
    }

    fn data_source() -> DataSource {
        DataSource::new("Münster".to_string(), "provider".to_string())
    }

    #[test]
    fn runs_when_job_has_never_succeeded() {
        let job_repo = Arc::new(MockJobRepository::new(Vec::new()));
        let data_source_repo = Arc::new(MockDataSourceRepository::new(Vec::new()));
        let service = service_with(job_repo.clone(), data_source_repo, Vec::new());

        service.run_if_due();

        let jobs = job_repo.jobs();
        assert_eq!(jobs.len(), 1);
        assert_eq!(jobs[0].job_type, DATA_SOURCE_UPDATE_JOB_TYPE);
        assert_eq!(jobs[0].status, JobStatus::Finished);
        assert!(!jobs[0].max_lifetime_exceeded);
    }

    #[test]
    fn skips_when_another_run_is_still_within_lifetime() {
        let now = Utc::now();
        let running = running_job(
            DATA_SOURCE_UPDATE_JOB_TYPE,
            now,
            now + Duration::seconds(3600),
        );
        let job_repo = Arc::new(MockJobRepository::new(vec![running]));
        let data_source_repo = Arc::new(MockDataSourceRepository::new(Vec::new()));
        let service = service_with(job_repo.clone(), data_source_repo, Vec::new());

        service.run_if_due();

        // No new job was created (the RUNNING job still blocks).
        assert_eq!(job_repo.jobs().len(), 1);
        assert_eq!(job_repo.jobs()[0].status, JobStatus::Running);
    }

    #[test]
    fn skips_when_job_already_succeeded_and_not_overdue() {
        let job_repo = Arc::new(MockJobRepository::new(vec![finished_job(
            DATA_SOURCE_UPDATE_JOB_TYPE,
        )]));
        let data_source_repo = Arc::new(MockDataSourceRepository::new(Vec::new()));
        let service = service_with(job_repo.clone(), data_source_repo, Vec::new());

        service.run_if_due();

        assert_eq!(
            job_repo.jobs().len(),
            1,
            "no new job after a recent success"
        );
    }

    #[test]
    fn runs_when_last_run_is_overdue() {
        // The last successful run finished ~2 hours ago: with the hourly cron
        // at least one trigger (the last hour boundary) has been missed, so the
        // job must run now.
        let finished = Utc::now() - Duration::hours(2);
        let job_repo = Arc::new(MockJobRepository::new(vec![finished_job_at(
            DATA_SOURCE_UPDATE_JOB_TYPE,
            finished,
        )]));
        let data_source_repo = Arc::new(MockDataSourceRepository::new(Vec::new()));
        let service = service_with(job_repo.clone(), data_source_repo, Vec::new());

        service.run_if_due();

        let jobs = job_repo.jobs();
        assert_eq!(jobs.len(), 2, "seeded finished job + a new overdue run");
        assert_eq!(jobs[0].status, JobStatus::Finished);
        assert_eq!(jobs[1].status, JobStatus::Finished);
    }

    #[test]
    fn runs_on_cron_tick() {
        let job_repo = Arc::new(MockJobRepository::new(Vec::new()));
        let data_source_repo = Arc::new(MockDataSourceRepository::new(Vec::new()));
        let service = service_with(job_repo.clone(), data_source_repo, Vec::new());

        service.run_if_due();

        let jobs = job_repo.jobs();
        assert_eq!(jobs.len(), 1);
        assert_eq!(jobs[0].status, JobStatus::Finished);
    }

    #[test]
    fn expires_stale_running_job_then_runs() {
        let now = Utc::now();
        // A RUNNING job past its lifetime_until: the scheduler must expire it
        // (FAILED + boolean) and then run a fresh job.
        let stale = running_job(
            DATA_SOURCE_UPDATE_JOB_TYPE,
            now - Duration::seconds(7200),
            now - Duration::seconds(60),
        );
        let job_repo = Arc::new(MockJobRepository::new(vec![stale]));
        let data_source_repo = Arc::new(MockDataSourceRepository::new(Vec::new()));
        let service = service_with(job_repo.clone(), data_source_repo, Vec::new());

        service.run_if_due();

        let jobs = job_repo.jobs();
        assert_eq!(jobs.len(), 2, "stale job expired + new job inserted");
        let stale_job = jobs
            .iter()
            .find(|job| job.status == JobStatus::Failed)
            .unwrap();
        assert!(stale_job.max_lifetime_exceeded);
        assert_eq!(
            stale_job.failure_message.as_deref(),
            Some("Max lifetime exceeded")
        );
        assert!(jobs.iter().any(|job| job.status == JobStatus::Finished));
    }

    #[test]
    fn marks_job_failed_when_start_fails() {
        let mut job_repo = MockJobRepository::new(Vec::new());
        job_repo.set_fail_set_running(true);
        let job_repo = Arc::new(job_repo);
        let data_source_repo = Arc::new(MockDataSourceRepository::new(Vec::new()));
        let service = service_with(job_repo.clone(), data_source_repo, Vec::new());

        service.run_if_due();

        // PENDING -> FAILED is a valid transition.
        let jobs = job_repo.jobs();
        assert_eq!(jobs.len(), 1);
        assert_eq!(jobs[0].status, JobStatus::Failed);
        assert!(jobs[0].failure_message.is_some());
    }

    #[test]
    fn run_if_due_handles_expire_running_jobs_error() {
        let mut job_repo = MockJobRepository::new(Vec::new());
        job_repo.set_fail_expire(true);
        let job_repo = Arc::new(job_repo);
        let data_source_repo = Arc::new(MockDataSourceRepository::new(Vec::new()));
        let service = service_with(job_repo.clone(), data_source_repo, Vec::new());

        // The expire error is logged and the service still proceeds: the job has
        // never succeeded, so a new run starts anyway.
        service.run_if_due();
        assert_eq!(job_repo.jobs().len(), 1);
    }

    #[test]
    fn marks_job_failed_when_update_fails() {
        let job_repo = Arc::new(MockJobRepository::new(Vec::new()));
        let mut data_source_repo = MockDataSourceRepository::new(vec![data_source()]);
        data_source_repo.set_fail_find(true);
        let data_source_repo = Arc::new(data_source_repo);
        let provider: Arc<dyn DataProvider> = Arc::new(ScriptedProvider {
            stations: Vec::new(),
            channels: Vec::new(),
            batches: Mutex::new(VecDeque::new()),
        });
        let service = service_with(job_repo.clone(), data_source_repo, vec![runtime(provider)]);

        service.run_if_due();

        // RUNNING -> FAILED.
        let jobs = job_repo.jobs();
        assert_eq!(jobs.len(), 1);
        assert_eq!(jobs[0].status, JobStatus::Failed);
        assert!(jobs[0].failure_message.is_some());
        assert!(!jobs[0].max_lifetime_exceeded);
    }

    #[test]
    fn records_progress_and_advances_imported_until() {
        let t0 = DateTime::parse_from_rfc3339("2024-01-01T00:00:00Z")
            .unwrap()
            .with_timezone(&Utc);
        let provider: Arc<dyn DataProvider> = Arc::new(ScriptedProvider {
            stations: vec![station_record("station-1")],
            channels: vec![channel_record("channel-1", "station-1")],
            batches: Mutex::new(VecDeque::from([SourceMeasurementBatch {
                measurements: vec![SourceMeasurement {
                    channel_external_id: "channel-1".to_string(),
                    record: measurement_record(1, t0),
                }],
                next_from: Some(t0),
                more: false,
            }])),
        });

        let job_repo = Arc::new(MockJobRepository::new(Vec::new()));
        let data_source_repo = Arc::new(MockDataSourceRepository::new(vec![data_source()]));
        let service = service_with(
            job_repo.clone(),
            data_source_repo.clone(),
            vec![runtime(provider)],
        );

        service.run_if_due();

        let jobs = job_repo.jobs();
        assert_eq!(jobs.len(), 1);
        assert_eq!(jobs[0].status, JobStatus::Finished);
        let source_id = DataSourceId(DataSource::id_from_name("Münster"));
        assert_eq!(
            jobs[0].metadata.get(&processed_measurements_key(source_id)),
            Some(&serde_json::json!(1))
        );
        assert_eq!(
            jobs[0].metadata.get(&added_measurements_key(source_id)),
            Some(&serde_json::json!(1))
        );
        assert_eq!(
            jobs[0].metadata.get(&status_key(source_id)),
            Some(&serde_json::json!("FINISHED")),
            "a finished source records its per-source FINISHED status"
        );

        let stored = data_source_repo
            .find_by_id(DataSourceId(DataSource::id_from_name("Münster")))
            .unwrap()
            .unwrap();
        assert_eq!(stored.imported_until, Some(t0));
        assert!(
            stored.last_updated_at.is_some(),
            "a successful source update stamps last_updated_at (drives the header)"
        );
    }

    #[test]
    fn run_updates_persists_measurement_bounds() {
        let t0 = DateTime::parse_from_rfc3339("2024-01-01T00:00:00Z")
            .unwrap()
            .with_timezone(&Utc);
        let t1 = DateTime::parse_from_rfc3339("2024-06-01T00:00:00Z")
            .unwrap()
            .with_timezone(&Utc);
        let provider: Arc<dyn DataProvider> = Arc::new(ScriptedProvider {
            stations: vec![station_record("station-1")],
            channels: vec![channel_record("channel-1", "station-1")],
            batches: Mutex::new(VecDeque::from([SourceMeasurementBatch {
                measurements: vec![
                    SourceMeasurement {
                        channel_external_id: "channel-1".to_string(),
                        record: measurement_record(1, t0),
                    },
                    SourceMeasurement {
                        channel_external_id: "channel-1".to_string(),
                        record: measurement_record(1, t1),
                    },
                ],
                next_from: Some(t1),
                more: false,
            }])),
        });

        let job_repo = Arc::new(MockJobRepository::new(Vec::new()));
        let data_source_repo = Arc::new(MockDataSourceRepository::new(vec![data_source()]));
        let service = service_with(
            job_repo.clone(),
            data_source_repo.clone(),
            vec![runtime(provider)],
        );

        service.run_if_due();

        let jobs = job_repo.jobs();
        assert_eq!(jobs.len(), 1);
        assert_eq!(jobs[0].status, JobStatus::Finished);

        let stored = data_source_repo
            .find_by_id(DataSourceId(DataSource::id_from_name("Münster")))
            .unwrap()
            .unwrap();
        assert_eq!(
            stored.first_measurement_at,
            Some(t0),
            "the run's earliest measurement is persisted as the lower bound"
        );
        assert_eq!(
            stored.last_measurement_at,
            Some(t1),
            "the run's latest measurement is persisted as the upper bound"
        );
    }

    #[test]
    fn run_updates_propagates_when_measurement_bounds_update_fails() {
        let job_repo = Arc::new(MockJobRepository::new(Vec::new()));
        let mut data_source_repo = MockDataSourceRepository::new(vec![data_source()]);
        data_source_repo.set_fail_update_measurement_bounds(true);
        let data_source_repo = Arc::new(data_source_repo);
        let t0 = DateTime::parse_from_rfc3339("2024-01-01T00:00:00Z")
            .unwrap()
            .with_timezone(&Utc);
        let service = service_with(
            job_repo.clone(),
            data_source_repo,
            vec![measured_runtime(t0)],
        );

        service.run_if_due();

        let jobs = job_repo.jobs();
        assert_eq!(jobs.len(), 1);
        assert_eq!(jobs[0].status, JobStatus::Failed);
        assert!(jobs[0].failure_message.is_some());
    }

    /// A runtime whose provider serves one measurement for channel-1 at `t0`
    /// with a real watermark, so `run_updates` tries to checkpoint it.
    fn measured_runtime(t0: DateTime<Utc>) -> DataSourceRuntime {
        let provider: Arc<dyn DataProvider> = Arc::new(ScriptedProvider {
            stations: vec![station_record("station-1")],
            channels: vec![channel_record("channel-1", "station-1")],
            batches: Mutex::new(VecDeque::from([SourceMeasurementBatch {
                measurements: vec![SourceMeasurement {
                    channel_external_id: "channel-1".to_string(),
                    record: measurement_record(1, t0),
                }],
                next_from: Some(t0),
                more: false,
            }])),
        });
        runtime(provider)
    }

    #[test]
    fn run_if_due_logs_and_returns_when_running_check_fails() {
        let mut job_repo = MockJobRepository::new(Vec::new());
        job_repo.set_fail_find_running(true);
        let job_repo = Arc::new(job_repo);
        let data_source_repo = Arc::new(MockDataSourceRepository::new(Vec::new()));
        let service = service_with(job_repo.clone(), data_source_repo, Vec::new());

        service.run_if_due();

        // The running-check error is logged and no run starts.
        assert!(job_repo.jobs().is_empty());
    }

    #[test]
    fn run_if_due_logs_when_last_finished_check_fails() {
        let mut job_repo = MockJobRepository::new(Vec::new());
        job_repo.set_fail_find_last_finished(true);
        let job_repo = Arc::new(job_repo);
        let data_source_repo = Arc::new(MockDataSourceRepository::new(Vec::new()));
        let service = service_with(job_repo.clone(), data_source_repo, Vec::new());

        service.run_if_due();

        // The last-finished error is logged and no run starts.
        assert!(job_repo.jobs().is_empty());
    }

    #[test]
    fn execute_logs_when_job_insert_fails() {
        let mut job_repo = MockJobRepository::new(Vec::new());
        job_repo.set_fail_insert(true);
        let job_repo = Arc::new(job_repo);
        let data_source_repo = Arc::new(MockDataSourceRepository::new(Vec::new()));
        let service = service_with(job_repo.clone(), data_source_repo, Vec::new());

        service.run_if_due();

        // The insert failure is logged; no job was recorded.
        assert!(job_repo.jobs().is_empty());
    }

    #[test]
    fn execute_logs_when_failed_marking_fails_after_start_failure() {
        let mut job_repo = MockJobRepository::new(Vec::new());
        job_repo.set_fail_set_running(true);
        job_repo.set_fail_set_failed(true);
        let job_repo = Arc::new(job_repo);
        let data_source_repo = Arc::new(MockDataSourceRepository::new(Vec::new()));
        let service = service_with(job_repo.clone(), data_source_repo, Vec::new());

        service.run_if_due();

        // The job stays PENDING because both the start and the failed-marking fail.
        let jobs = job_repo.jobs();
        assert_eq!(jobs.len(), 1);
        assert_eq!(jobs[0].status, JobStatus::Pending);
    }

    #[test]
    fn execute_logs_when_finish_marking_fails() {
        let mut job_repo = MockJobRepository::new(Vec::new());
        job_repo.set_fail_set_finished(true);
        let job_repo = Arc::new(job_repo);
        let data_source_repo = Arc::new(MockDataSourceRepository::new(Vec::new()));
        let service = service_with(job_repo.clone(), data_source_repo, Vec::new());

        service.run_if_due();

        // run_updates succeeds but the FINISHED marking fails: the job stays RUNNING.
        let jobs = job_repo.jobs();
        assert_eq!(jobs.len(), 1);
        assert_eq!(jobs[0].status, JobStatus::Running);
    }

    #[test]
    fn execute_logs_when_failed_marking_fails_after_update_failure() {
        let mut job_repo = MockJobRepository::new(Vec::new());
        job_repo.set_fail_set_failed(true);
        let job_repo = Arc::new(job_repo);
        let mut data_source_repo = MockDataSourceRepository::new(vec![data_source()]);
        data_source_repo.set_fail_find(true);
        let data_source_repo = Arc::new(data_source_repo);
        let provider: Arc<dyn DataProvider> = Arc::new(ScriptedProvider {
            stations: Vec::new(),
            channels: Vec::new(),
            batches: Mutex::new(VecDeque::new()),
        });
        let service = service_with(job_repo.clone(), data_source_repo, vec![runtime(provider)]);

        service.run_if_due();

        // The update failed and the FAILED marking itself failed: the job stays
        // RUNNING (the error was only logged).
        let jobs = job_repo.jobs();
        assert_eq!(jobs.len(), 1);
        assert_eq!(jobs[0].status, JobStatus::Running);
    }

    #[test]
    fn run_updates_logs_when_import_run_insert_fails() {
        let job_repo = Arc::new(MockJobRepository::new(Vec::new()));
        let data_source_repo = Arc::new(MockDataSourceRepository::new(vec![data_source()]));
        let import_runs = Arc::new(MemoryImportRunRepository::default());
        import_runs.set_fail_insert(true);
        let t0 = DateTime::parse_from_rfc3339("2024-01-01T00:00:00Z")
            .unwrap()
            .with_timezone(&Utc);
        let service = service_with_import_runs(
            job_repo.clone(),
            data_source_repo,
            import_runs,
            vec![measured_runtime(t0)],
        );

        service.run_if_due();

        // The run-record insert failure is non-fatal: the source still updates.
        let jobs = job_repo.jobs();
        assert_eq!(jobs.len(), 1);
        assert_eq!(jobs[0].status, JobStatus::Finished);
    }

    #[test]
    fn run_updates_propagates_when_watermark_checkpoint_fails() {
        let job_repo = Arc::new(MockJobRepository::new(Vec::new()));
        let mut data_source_repo = MockDataSourceRepository::new(vec![data_source()]);
        data_source_repo.set_fail_update_imported_until(true);
        let data_source_repo = Arc::new(data_source_repo);
        let t0 = DateTime::parse_from_rfc3339("2024-01-01T00:00:00Z")
            .unwrap()
            .with_timezone(&Utc);
        let service = service_with(
            job_repo.clone(),
            data_source_repo,
            vec![measured_runtime(t0)],
        );

        service.run_if_due();

        // The per-batch checkpoint error aborts the source update -> RUNNING -> FAILED.
        let jobs = job_repo.jobs();
        assert_eq!(jobs.len(), 1);
        assert_eq!(jobs[0].status, JobStatus::Failed);
        assert!(jobs[0].failure_message.is_some());
    }

    #[test]
    fn run_updates_propagates_when_last_updated_checkpoint_fails() {
        let job_repo = Arc::new(MockJobRepository::new(Vec::new()));
        let mut data_source_repo = MockDataSourceRepository::new(vec![data_source()]);
        data_source_repo.set_fail_update_last_updated(true);
        let data_source_repo = Arc::new(data_source_repo);
        let t0 = DateTime::parse_from_rfc3339("2024-01-01T00:00:00Z")
            .unwrap()
            .with_timezone(&Utc);
        let service = service_with(
            job_repo.clone(),
            data_source_repo,
            vec![measured_runtime(t0)],
        );

        service.run_if_due();

        let jobs = job_repo.jobs();
        assert_eq!(jobs.len(), 1);
        assert_eq!(jobs[0].status, JobStatus::Failed);
    }

    #[test]
    fn run_updates_logs_when_import_run_finish_fails() {
        let job_repo = Arc::new(MockJobRepository::new(Vec::new()));
        let data_source_repo = Arc::new(MockDataSourceRepository::new(vec![data_source()]));
        let import_runs = Arc::new(MemoryImportRunRepository::default());
        import_runs.set_fail_finish(true);
        let t0 = DateTime::parse_from_rfc3339("2024-01-01T00:00:00Z")
            .unwrap()
            .with_timezone(&Utc);
        let service = service_with_import_runs(
            job_repo.clone(),
            data_source_repo,
            import_runs,
            vec![measured_runtime(t0)],
        );

        service.run_if_due();

        // The finish-failure is non-fatal (only logged).
        let jobs = job_repo.jobs();
        assert_eq!(jobs.len(), 1);
        assert_eq!(jobs[0].status, JobStatus::Finished);
    }

    #[test]
    fn run_updates_logs_when_run_failure_marking_fails() {
        let job_repo = Arc::new(MockJobRepository::new(Vec::new()));
        let mut data_source_repo = MockDataSourceRepository::new(vec![data_source()]);
        data_source_repo.set_fail_update_imported_until(true);
        let data_source_repo = Arc::new(data_source_repo);
        let import_runs = Arc::new(MemoryImportRunRepository::default());
        import_runs.set_fail_fail(true);
        let t0 = DateTime::parse_from_rfc3339("2024-01-01T00:00:00Z")
            .unwrap()
            .with_timezone(&Utc);
        let service = service_with_import_runs(
            job_repo.clone(),
            data_source_repo,
            import_runs,
            vec![measured_runtime(t0)],
        );

        service.run_if_due();

        // The update failed and even recording the run failure failed: the job
        // is still marked FAILED (the bookkeeping error is only logged).
        let jobs = job_repo.jobs();
        assert_eq!(jobs.len(), 1);
        assert_eq!(jobs[0].status, JobStatus::Failed);
    }

    /// Builds a runtime for a data source with the given name (the shared
    /// `runtime` helper is hard-wired to Münster).
    fn named_runtime(name: &str, provider: Arc<dyn DataProvider>) -> DataSourceRuntime {
        DataSourceRuntime {
            configuration: DataSourceConfiguration::new(
                name.to_string(),
                DataProviderConfiguration::new("provider".to_string(), HashMap::new()).unwrap(),
            )
            .unwrap(),
            data_source_id: DataSourceId(DataSource::id_from_name(name)),
            provider,
        }
    }

    /// A provider whose measurement read bumps a shared "active" counter while
    /// sleeping, so a test can observe that two sources really overlap.
    struct ConcurrencyProvider {
        active: Arc<Mutex<usize>>,
        max_active: Arc<Mutex<usize>>,
    }

    impl DataProvider for ConcurrencyProvider {
        fn check_health(&self) -> HealthStatus {
            HealthStatus::Up
        }

        fn get_all_counting_stations(&self) -> Result<Vec<CountingStationRecord>, ProviderError> {
            Ok(Vec::new())
        }

        fn get_all_channels(&self) -> Result<Vec<ChannelRecord>, ProviderError> {
            Ok(Vec::new())
        }

        fn get_measurements_source(
            &self,
            _from: Option<DateTime<Utc>>,
            _max_batch_size: usize,
        ) -> Result<SourceMeasurementBatch, ProviderError> {
            {
                let mut active = self.active.lock().unwrap();
                *active += 1;
                let mut max_active = self.max_active.lock().unwrap();
                if *active > *max_active {
                    *max_active = *active;
                }
            }
            // Keep every worker inside the measurement read long enough that a
            // sequential implementation would never see them overlap.
            std::thread::sleep(std::time::Duration::from_millis(100));
            {
                let mut active = self.active.lock().unwrap();
                *active -= 1;
            }
            Ok(SourceMeasurementBatch {
                measurements: Vec::new(),
                next_from: None,
                more: false,
            })
        }

        fn max_measurement_batch_size(&self) -> usize {
            100
        }
    }

    #[test]
    fn runs_all_data_sources_in_parallel() {
        let job_repo = Arc::new(MockJobRepository::new(Vec::new()));
        let data_source_repo = Arc::new(MockDataSourceRepository::new(Vec::new()));
        let active = Arc::new(Mutex::new(0usize));
        let max_active = Arc::new(Mutex::new(0usize));
        let make = || ConcurrencyProvider {
            active: active.clone(),
            max_active: max_active.clone(),
        };
        let service = service_with(
            job_repo.clone(),
            data_source_repo,
            vec![
                named_runtime("Münster", Arc::new(make())),
                named_runtime("Bonn", Arc::new(make())),
            ],
        );

        service.run_if_due();

        let jobs = job_repo.jobs();
        assert_eq!(jobs.len(), 1, "one aggregate job covers all sources");
        assert_eq!(jobs[0].status, JobStatus::Finished);
        assert_eq!(
            *max_active.lock().unwrap(),
            2,
            "both data sources must be importing at the same time"
        );
        let muenster = DataSourceId(DataSource::id_from_name("Münster"));
        let bonn = DataSourceId(DataSource::id_from_name("Bonn"));
        assert_eq!(
            jobs[0].metadata.get(&status_key(muenster)),
            Some(&serde_json::json!("FINISHED"))
        );
        assert_eq!(
            jobs[0].metadata.get(&status_key(bonn)),
            Some(&serde_json::json!("FINISHED"))
        );
    }

    #[test]
    fn failing_source_records_failed_status_while_others_finish() {
        let job_repo = Arc::new(MockJobRepository::new(Vec::new()));
        let data_source_repo = Arc::new(MockDataSourceRepository::new(Vec::new()));
        // Münster finishes with one empty batch; Bonn's provider errors (empty
        // batch queue -> "no more pages") so only that source fails.
        let ok_provider: Arc<dyn DataProvider> = Arc::new(ScriptedProvider {
            stations: vec![station_record("station-1")],
            channels: vec![channel_record("channel-1", "station-1")],
            batches: Mutex::new(VecDeque::from([SourceMeasurementBatch {
                measurements: Vec::new(),
                next_from: None,
                more: false,
            }])),
        });
        let failing_provider: Arc<dyn DataProvider> = Arc::new(ScriptedProvider {
            stations: vec![station_record("station-2")],
            channels: vec![channel_record("channel-2", "station-2")],
            batches: Mutex::new(VecDeque::new()),
        });
        let service = service_with(
            job_repo.clone(),
            data_source_repo,
            vec![
                named_runtime("Münster", ok_provider),
                named_runtime("Bonn", failing_provider),
            ],
        );

        service.run_if_due();

        let jobs = job_repo.jobs();
        assert_eq!(jobs.len(), 1);
        assert_eq!(
            jobs[0].status,
            JobStatus::Failed,
            "any failing source still fails the aggregate job"
        );
        assert!(jobs[0].failure_message.is_some());
        let muenster = DataSourceId(DataSource::id_from_name("Münster"));
        let bonn = DataSourceId(DataSource::id_from_name("Bonn"));
        assert_eq!(
            jobs[0].metadata.get(&status_key(muenster)),
            Some(&serde_json::json!("FINISHED")),
            "the successful source still finishes while the other fails"
        );
        assert_eq!(
            jobs[0].metadata.get(&status_key(bonn)),
            Some(&serde_json::json!("FAILED"))
        );
    }
}
