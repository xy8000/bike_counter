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

use crate::core::application::data_import_service::{DataImportService, DataSourceRuntime};
use crate::core::domain::configuration::configuration::Configuration;
use crate::core::domain::data_source::repository_port::DataSourceRepository;
use crate::core::domain::data_source::service_port::DataSourceUpdateServicePort;
use crate::core::domain::error::DomainError;
use crate::core::domain::jobs::job::Job;
use crate::core::domain::jobs::repository_port::JobRepository;

/// The job type owned by this service.
pub const DATA_SOURCE_UPDATE_JOB_TYPE: &str = "data_source_update";
/// Human-readable name of the data-source update job.
pub const DATA_SOURCE_UPDATE_JOB_NAME: &str = "Data source update";
/// Job metadata key holding the running processed-measurement count (rows read
/// from the provider).
pub const PROCESSED_MEASUREMENTS_KEY: &str = "processed_measurements";
/// Job metadata key holding the running added-measurement count (rows actually
/// inserted).
pub const ADDED_MEASUREMENTS_KEY: &str = "added_measurements";

pub struct DataSourceUpdateService {
    job_repository: Arc<dyn JobRepository + Send + Sync>,
    data_source_repository: Arc<dyn DataSourceRepository + Send + Sync>,
    data_import_service: Arc<DataImportService>,
    configuration: Arc<Configuration>,
    runtimes: Vec<DataSourceRuntime>,
}

impl DataSourceUpdateService {
    pub fn new(
        job_repository: Arc<dyn JobRepository + Send + Sync>,
        data_source_repository: Arc<dyn DataSourceRepository + Send + Sync>,
        data_import_service: Arc<DataImportService>,
        configuration: Arc<Configuration>,
        runtimes: Vec<DataSourceRuntime>,
    ) -> Self {
        Self {
            job_repository,
            data_source_repository,
            data_import_service,
            configuration,
            runtimes,
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

        // 1. Expire stale RUNNING jobs (past their lifetime_until deadline).
        match self
            .job_repository
            .expire_running_jobs(DATA_SOURCE_UPDATE_JOB_TYPE, now)
        {
            Ok(expired) if expired > 0 => {
                println!("Expired {expired} stale RUNNING data-source update job(s)");
            }
            Ok(_) => {}
            Err(error) => {
                eprintln!("Failed to expire stale data-source update jobs: {error:?}");
            }
        }

        // 2. A RUNNING job within its lifetime blocks a new run.
        match self
            .job_repository
            .find_running_by_type(DATA_SOURCE_UPDATE_JOB_TYPE)
        {
            Ok(Some(running)) if !running.lifetime_exceeded(now) => {
                println!(
                    "Data source update job {} is still running (until {}); skipping",
                    running.id, running.lifetime_until
                );
                return;
            }
            Ok(_) => {}
            Err(error) => {
                eprintln!("Failed to check for a running data-source update job: {error:?}");
                return;
            }
        }

        // 3. Run when the job has never succeeded or the last successful run is
        //    overdue.
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

    /// Runs one full data-source update as a tracked job.
    fn execute(&self, now: DateTime<Utc>) {
        let job = Job::new(
            Uuid::new_v4(),
            DATA_SOURCE_UPDATE_JOB_NAME.to_string(),
            DATA_SOURCE_UPDATE_JOB_TYPE.to_string(),
            now + self.configuration.data_source_update_max_lifetime(),
        );
        let job_id = job.id;
        let job_name = job.name.clone();

        if let Err(error) = self.job_repository.insert(job) {
            eprintln!("Failed to record data-source update job {job_name} ({job_id}): {error:?}");
            return;
        }

        // PENDING -> RUNNING.
        if let Err(error) = self.job_repository.set_running(job_id, now) {
            // PENDING -> FAILED (a job may fail before it ever starts).
            let message = format!("failed to start job: {error:?}");
            if let Err(fail_error) = self.job_repository.set_failed(job_id, Utc::now(), &message) {
                eprintln!(
                    "Failed to mark data-source update job {job_name} ({job_id}) as failed: {fail_error:?}"
                );
            }
            return;
        }

        println!("Data source update job {job_name} ({job_id}) started");

        // RUNNING -> FINISHED / FAILED.
        match self.run_updates(job_id) {
            Ok(()) => {
                if let Err(error) = self.job_repository.set_finished(job_id, Utc::now()) {
                    eprintln!(
                        "Failed to finish data-source update job {job_name} ({job_id}): {error:?}"
                    );
                } else {
                    println!("Data source update job {job_name} ({job_id}) finished");
                }
            }
            Err(error) => {
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
    }

    /// Updates every configured data source, in strict order per source:
    /// counting stations, then channels, then measurements (paged from the
    /// source's `imported_until`). After each persisted measurement batch the
    /// running `processed_measurements` and `added_measurements` counts are
    /// recorded in the job metadata. On success the data source's
    /// `imported_until` is advanced to the last processed measurement timestamp
    /// (incremental, no reprocessing).
    fn run_updates(&self, job_id: Uuid) -> Result<(), DomainError> {
        for runtime in &self.runtimes {
            let from = self
                .data_source_repository
                .find_by_id(runtime.data_source_id)?
                .and_then(|data_source| data_source.imported_until);

            let update = self.data_import_service.update_data_source(
                runtime,
                from,
                |processed, added| {
                    self.job_repository.update_metadata(
                        job_id,
                        PROCESSED_MEASUREMENTS_KEY,
                        json!(processed),
                    )?;
                    self.job_repository.update_metadata(
                        job_id,
                        ADDED_MEASUREMENTS_KEY,
                        json!(added),
                    )
                },
            )?;

            if let Some(last_timestamp) = update.last_measurement_timestamp {
                self.data_source_repository
                    .update_imported_until(runtime.data_source_id, last_timestamp)?;
            }
        }
        Ok(())
    }
}

impl DataSourceUpdateServicePort for DataSourceUpdateService {
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
        DataProviderConfiguration, DataSourceConfiguration, DatabaseConfiguration,
    };
    use crate::core::domain::configuration::configuration::{
        Configuration, DEFAULT_DATA_SOURCE_UPDATE_CRON,
    };
    use crate::core::domain::counting_stations::counting_station::CountingStation;
    use crate::core::domain::counting_stations::counting_station::value_objects as station_vo;
    use crate::core::domain::counting_stations::repository_port::CountingStationRepository;
    use crate::core::domain::data_source::data_source::DataSource;
    use crate::core::domain::data_source::data_source::value_objects::Id as DataSourceId;
    use crate::core::domain::data_source::provider_port::{
        ChannelRecord, CountingStationRecord, DataProvider, MeasurementBatch, MeasurementQuery,
        MeasurementRecord, ProviderError,
    };
    use crate::core::domain::data_source::repository_port::DataSourceRepository;
    use crate::core::domain::health::HealthStatus;
    use crate::core::domain::jobs::job::JobStatus;
    use crate::core::domain::measurements::measurement::Measurement;
    use crate::core::domain::measurements::measurement::value_objects as measurement_vo;
    use crate::core::domain::measurements::repository_port::MeasurementRepository;

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
        MeasurementRecord { value, timestamp }
    }

    /// An in-memory job repository that records the full lifecycle.
    struct MockJobRepository {
        jobs: Mutex<Vec<Job>>,
        fail_set_running: bool,
    }

    impl MockJobRepository {
        fn new(jobs: Vec<Job>) -> Self {
            Self {
                jobs: Mutex::new(jobs),
                fail_set_running: false,
            }
        }

        fn set_fail_set_running(&mut self, fail: bool) {
            self.fail_set_running = fail;
        }

        fn jobs(&self) -> Vec<Job> {
            self.jobs.lock().unwrap().clone()
        }
    }

    impl JobRepository for MockJobRepository {
        fn insert(&self, job: Job) -> Result<(), DomainError> {
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
            job_type: &str,
            now: DateTime<Utc>,
        ) -> Result<u64, DomainError> {
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
    }

    impl MockDataSourceRepository {
        fn new(data_sources: Vec<DataSource>) -> Self {
            Self {
                data_sources: Mutex::new(data_sources),
                fail_find: false,
            }
        }

        fn set_fail_find(&mut self, fail: bool) {
            self.fail_find = fail;
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
    }

    /// A provider that serves fixed external-id records and a single page queue.
    struct ScriptedProvider {
        stations: Vec<CountingStationRecord>,
        channels: Vec<ChannelRecord>,
        pages: Mutex<VecDeque<MeasurementBatch>>,
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

        fn get_measurements(
            &self,
            _query: MeasurementQuery,
        ) -> Result<MeasurementBatch, ProviderError> {
            self.pages
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

    /// A service with no configured data sources (empty runtimes).
    fn service_with(
        job_repo: Arc<MockJobRepository>,
        data_source_repo: Arc<MockDataSourceRepository>,
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
            data_import,
            Arc::new(configuration()),
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
    fn marks_job_failed_when_update_fails() {
        let job_repo = Arc::new(MockJobRepository::new(Vec::new()));
        let mut data_source_repo = MockDataSourceRepository::new(vec![data_source()]);
        data_source_repo.set_fail_find(true);
        let data_source_repo = Arc::new(data_source_repo);
        let provider: Arc<dyn DataProvider> = Arc::new(ScriptedProvider {
            stations: Vec::new(),
            channels: Vec::new(),
            pages: Mutex::new(VecDeque::new()),
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
            pages: Mutex::new(VecDeque::from([MeasurementBatch {
                measurements: vec![measurement_record(1, t0)],
                last_measurement_datetime: Some(t0),
                batch_size_limit_reached: false,
                timeframe_limit_reached: false,
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
        assert_eq!(
            jobs[0].metadata.get(PROCESSED_MEASUREMENTS_KEY),
            Some(&serde_json::json!(1))
        );
        assert_eq!(
            jobs[0].metadata.get(ADDED_MEASUREMENTS_KEY),
            Some(&serde_json::json!(1))
        );

        let stored = data_source_repo
            .find_by_id(DataSourceId(DataSource::id_from_name("Münster")))
            .unwrap()
            .unwrap();
        assert_eq!(stored.imported_until, Some(t0));
    }
}
