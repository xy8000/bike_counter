//! Application job runner that publishes the processed measurements as
//! immutable OpenData files (parquet / csv.gz / json) into the dedicated
//! opendata object-storage bucket, one file per global and per-station
//! daily/monthly period.
//!
//! Mirrors [`AssetCleanupService`]'s scheduling: it is driven by the generic
//! cron scheduler through [`ScheduledJobPort`] and tracks itself as an
//! `opendata_export` job. Multi-instance cancellation follows the shared
//! protocol (claim the type's `job_locks` row, record a RUNNING job owned by
//! this instance, heartbeat after each sub-task and honor a cancellation
//! request).
//!
//! The files are **append-only**: the registry (`opendata_files`) is both the
//! ledger and the persisted state. Each run computes the complete periods
//! (`Europe/Berlin` calendar days strictly before today, calendar months before
//! the current month) that are not yet registered and appends the missing ones.
//! Nothing is ever overwritten.

use std::str::FromStr;
use std::sync::Arc;

use chrono::{DateTime, Datelike, Duration, NaiveDate, TimeZone, Utc};
use chrono_tz::Tz;
use sha2::{Digest, Sha256 as Sha2Digest};
use uuid::Uuid;

use super::job_heartbeat::JobHeartbeat;
use crate::core::domain::assets::asset::value_objects::{ContentType, ObjectKey};
use crate::core::domain::assets::asset_storage_port::AssetStorage;
use crate::core::domain::configuration::configuration::Configuration;
use crate::core::domain::counting_stations::counting_station::value_objects::Status;
use crate::core::domain::counting_stations::repository_port::CountingStationRepository;
use crate::core::domain::error::DomainError;
use crate::core::domain::jobs::job::{Job, JobStatus};
use crate::core::domain::jobs::repository_port::JobRepository;
use crate::core::domain::jobs::scheduled_job_port::ScheduledJobPort;
use crate::core::domain::opendata::file::{Format, Granularity, OpenDataFile, object_key};
use crate::core::domain::opendata::file_generator_port::OpenDataFileGenerator;
use crate::core::domain::opendata::file_repository_port::OpenDataFileRepository;
use crate::core::domain::opendata::measurement_reader_port::OpenDataMeasurementReader;
use serde_json::json;

/// The job type owned by this service.
pub const OPENDATA_EXPORT_JOB_TYPE: &str = "opendata_export";
/// Human-readable name of the opendata export job.
pub const OPENDATA_EXPORT_JOB_NAME: &str = "opendata export";
/// The timezone all files are bucketed and timestamped in (central Europe, no
/// UTC offset in the serialized timestamps).
pub const OPENDATA_TIMEZONE: &str = "Europe/Berlin";
/// Job-metadata key recording how many files this run created (published). The
/// value is written when the job starts (0) and kept up to date after every
/// export scope, so the job-info always reports the files created so far; the
/// finalize step overwrites it with the run's final total.
pub const FILES_CREATED_KEY: &str = "files_created";
/// Backfill lower bound: no provider predates 2000, so the first run exports
/// everything with data from 2000-01-01 up to the latest complete period.
const EXPORT_EPOCH: (i32, u32, u32) = (2000, 1, 1);

pub struct OpenDataExportService {
    job_repository: Arc<dyn JobRepository + Send + Sync>,
    file_repository: Arc<dyn OpenDataFileRepository>,
    measurement_reader: Arc<dyn OpenDataMeasurementReader>,
    generator: Arc<dyn OpenDataFileGenerator>,
    storage: Arc<dyn AssetStorage>,
    counting_station_repository: Arc<dyn CountingStationRepository + Send + Sync>,
    configuration: Arc<Configuration>,
    instance_id: Uuid,
}

impl OpenDataExportService {
    #[allow(clippy::too_many_arguments)]
    pub fn new(
        job_repository: Arc<dyn JobRepository + Send + Sync>,
        file_repository: Arc<dyn OpenDataFileRepository>,
        measurement_reader: Arc<dyn OpenDataMeasurementReader>,
        generator: Arc<dyn OpenDataFileGenerator>,
        storage: Arc<dyn AssetStorage>,
        counting_station_repository: Arc<dyn CountingStationRepository + Send + Sync>,
        configuration: Arc<Configuration>,
        instance_id: Uuid,
    ) -> Self {
        Self {
            job_repository,
            file_repository,
            measurement_reader,
            generator,
            storage,
            counting_station_repository,
            configuration,
            instance_id,
        }
    }

    /// Decides whether the opendata export should run now and executes it if so.
    /// Same always-on rule as the other jobs: run at startup (never succeeded)
    /// and whenever the last successful run is overdue; skip while an active
    /// job is still in flight or being finalized.
    pub fn run_if_due(&self) {
        let now = Utc::now();

        match self
            .job_repository
            .find_active_by_type(OPENDATA_EXPORT_JOB_TYPE)
        {
            Ok(active) if !active.is_empty() => {
                let count = active.len();
                let ids = active
                    .iter()
                    .map(|job| job.id.to_string())
                    .collect::<Vec<_>>()
                    .join(", ");
                println!(
                    "OpenData export job is still active ({count} running/requesting: {ids}); \
                     skipping"
                );
                return;
            }
            Ok(_) => {}
            Err(error) => {
                eprintln!("Failed to check for an active opendata export job: {error:?}");
                return;
            }
        }

        match self
            .job_repository
            .find_last_finished_by_type(OPENDATA_EXPORT_JOB_TYPE)
        {
            Ok(None) => {
                println!("OpenData export job has never succeeded; running");
                self.execute(now);
            }
            Ok(Some(last)) => {
                if self.is_overdue(&last, now) {
                    println!(
                        "OpenData export job is overdue (last run {} at {}); running",
                        last.id,
                        last.finished_at
                            .map(|ts| ts.to_rfc3339())
                            .unwrap_or_else(|| "unknown".to_string())
                    );
                    self.execute(now);
                }
            }
            Err(error) => {
                eprintln!("Failed to check the last finished opendata export job: {error:?}");
            }
        }
    }

    /// Whether the last successful run is overdue: the next scheduled cron
    /// trigger after its finish time has already passed.
    fn is_overdue(&self, last: &Job, now: DateTime<Utc>) -> bool {
        let schedule = match cron::Schedule::from_str(self.configuration.opendata_export_cron()) {
            Ok(schedule) => schedule,
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

    /// Runs one export pass as a job owned by this instance.
    fn execute(&self, now: DateTime<Utc>) {
        let interval = self.configuration.opendata_export_max_heartbeat_interval();
        let instance_id = self.instance_id;

        // 1. Claim the type's lock; only the winning instance proceeds.
        match self
            .job_repository
            .acquire(OPENDATA_EXPORT_JOB_TYPE, instance_id, now + interval)
        {
            Ok(true) => {}
            Ok(false) => {
                println!("OpenData export is already active elsewhere (job_locks held); skipping");
                return;
            }
            Err(error) => {
                eprintln!("Failed to acquire the opendata export lock: {error:?}");
                return;
            }
        }

        // 2. Record the RUNNING job owned by this instance.
        let job = Job::running(
            Uuid::new_v4(),
            OPENDATA_EXPORT_JOB_NAME.to_string(),
            OPENDATA_EXPORT_JOB_TYPE.to_string(),
            instance_id,
            now,
        );
        let job_id = job.id;
        let job_name = job.name.clone();
        if let Err(error) = self.job_repository.insert(job) {
            let _ = self
                .job_repository
                .release(OPENDATA_EXPORT_JOB_TYPE, instance_id);
            eprintln!("Failed to record opendata export job {job_name} ({job_id}): {error:?}");
            return;
        }
        println!("OpenData export job {job_name} ({job_id}) started");
        // Expose the run on the job-info from the start (best-effort).
        self.record_files_created(job_id, 0);

        // 3. A dedicated heartbeat loop keeps the job fresh on a fixed tick,
        //    independent of how long an individual file generation takes.
        let heartbeat = JobHeartbeat::start(
            self.job_repository.clone(),
            job_id,
            OPENDATA_EXPORT_JOB_TYPE,
            instance_id,
            interval,
        );
        let outcome = self.run_export(job_id);
        heartbeat.stop();

        // 4. Finalize based on the resulting status.
        self.finalize(job_id, &job_name, outcome);
    }

    /// Finalizes the job according to `outcome` and the current persisted
    /// status, then always releases the type's lock.
    fn finalize(&self, job_id: Uuid, job_name: &str, outcome: Result<usize, DomainError>) {
        let status = self
            .job_repository
            .find_by_id(job_id)
            .ok()
            .flatten()
            .map(|job| job.status);
        match (outcome, status) {
            (_, Some(JobStatus::CancellationRequested)) | (_, Some(JobStatus::Cancelled)) => {
                match self.job_repository.mark_cancelled(job_id, Utc::now()) {
                    Ok(()) => println!("OpenData export job {job_name} ({job_id}) cancelled"),
                    Err(error) => eprintln!(
                        "Could not finalize opendata export job {job_name} ({job_id}) as cancelled: {error:?}"
                    ),
                }
            }
            (Ok(files_added), _) => {
                let _ = self.job_repository.update_metadata(
                    job_id,
                    FILES_CREATED_KEY,
                    json!(files_added),
                );
                if let Err(error) = self.job_repository.set_finished(job_id, Utc::now()) {
                    eprintln!(
                        "Failed to finish opendata export job {job_name} ({job_id}): {error:?}"
                    );
                } else {
                    println!(
                        "OpenData export job {job_name} ({job_id}) finished ({files_added} file(s))"
                    );
                }
            }
            (Err(error), _) => {
                let message = format!("{error:?}");
                if let Err(set_failed_error) =
                    self.job_repository.set_failed(job_id, Utc::now(), &message)
                {
                    eprintln!(
                        "Failed to mark opendata export job {job_name} ({job_id}) as failed: {set_failed_error:?}"
                    );
                } else {
                    eprintln!("OpenData export job {job_name} ({job_id}) failed: {error:?}");
                }
            }
        }
        let _ = self
            .job_repository
            .release(OPENDATA_EXPORT_JOB_TYPE, self.instance_id);
    }

    /// Runs the export: global daily + monthly files, then the daily + monthly
    /// files of every active station. Each period is a sub-task boundary: the
    /// job heartbeats and honors a cancellation request by returning
    /// [`DomainError::Cancelled`]. Returns the number of files appended.
    fn run_export(&self, job_id: Uuid) -> Result<usize, DomainError> {
        self.check_cancellation(job_id)?;
        let mut files_added = 0usize;

        self.export_scope(job_id, Granularity::Daily, None, &mut files_added)?;
        self.record_files_created(job_id, files_added);
        self.export_scope(job_id, Granularity::Monthly, None, &mut files_added)?;
        self.record_files_created(job_id, files_added);

        let active = self
            .counting_station_repository
            .find_all()?
            .into_iter()
            .filter(|station| station.status == Status::Active);
        for station in active {
            self.check_cancellation(job_id)?;
            self.export_scope(
                job_id,
                Granularity::Daily,
                Some(station.id.0),
                &mut files_added,
            )?;
            self.record_files_created(job_id, files_added);
            self.export_scope(
                job_id,
                Granularity::Monthly,
                Some(station.id.0),
                &mut files_added,
            )?;
            self.record_files_created(job_id, files_added);
        }

        Ok(files_added)
    }

    /// Best-effort metadata update exposing the files created so far on the
    /// job-info. Bookkeeping only: a failed write never aborts the export.
    fn record_files_created(&self, job_id: Uuid, files_added: usize) {
        if let Err(error) =
            self.job_repository
                .update_metadata(job_id, FILES_CREATED_KEY, json!(files_added))
        {
            eprintln!("Failed to record opendata export files-created metadata: {error:?}");
        }
    }

    /// Appends every missing complete period of one granularity/scope, each
    /// format per period, and then ensures the newest stored period is complete
    /// (a crashed run may have published only some of its formats).
    fn export_scope(
        &self,
        job_id: Uuid,
        granularity: Granularity,
        station_id: Option<Uuid>,
        files_added: &mut usize,
    ) -> Result<(), DomainError> {
        let now = Utc::now();
        let (first, latest_complete) = self.scope_window(granularity, station_id, now)?;

        if first <= latest_complete {
            let tz = Self::timezone()?;
            let from_utc = local_midnight_utc(tz, first)?;
            let to_utc = local_midnight_utc(tz, latest_complete + Duration::days(1))?
                - Duration::microseconds(1);

            // Only periods that actually contain measurements are candidates.
            let available = self.measurement_reader.available_periods(
                granularity,
                from_utc,
                to_utc,
                station_id,
            )?;

            let max_period = self.file_repository.max_period(granularity, station_id)?;
            for period in available {
                self.check_cancellation(job_id)?;
                if max_period
                    .as_ref()
                    .is_some_and(|max| period.as_str() <= max.as_str())
                {
                    continue;
                }
                *files_added += self.export_period(job_id, granularity, period, station_id)?;
            }
        }

        // Crash recovery: if the newest stored period was only partially
        // published (a run stopped mid-period), publish its missing formats.
        self.complete_newest_period(
            job_id,
            granularity,
            station_id,
            latest_complete,
            files_added,
        )
    }

    /// Fills any missing formats of the newest stored period when it is a
    /// complete past period (append-only: existing formats are never rewritten).
    fn complete_newest_period(
        &self,
        job_id: Uuid,
        granularity: Granularity,
        station_id: Option<Uuid>,
        latest_complete: NaiveDate,
        files_added: &mut usize,
    ) -> Result<(), DomainError> {
        let Some(max) = self.file_repository.max_period(granularity, station_id)? else {
            return Ok(());
        };
        let max_date = period_first_date(&max, granularity)?;
        if max_date > latest_complete {
            return Ok(()); // the newest stored period is not complete yet
        }
        self.check_cancellation(job_id)?;
        *files_added += self.export_period(job_id, granularity, max, station_id)?;
        Ok(())
    }

    /// The first candidate date and the latest complete date for a scope, in
    /// the export timezone. `first` is the day/month after the newest stored
    /// period (or the export epoch on a first run); `latest_complete` is
    /// yesterday for daily and the last day of the previous month for monthly.
    fn scope_window(
        &self,
        granularity: Granularity,
        station_id: Option<Uuid>,
        now: DateTime<Utc>,
    ) -> Result<(NaiveDate, NaiveDate), DomainError> {
        let tz = Self::timezone()?;
        let today = now.with_timezone(&tz).date_naive();
        let latest_complete = match granularity {
            Granularity::Daily => today - Duration::days(1),
            Granularity::Monthly => {
                let first_of_current = today.with_day(1).ok_or_else(|| {
                    DomainError::InvalidQuery(
                        "cannot compute the first day of the month".to_string(),
                    )
                })?;
                first_of_current - Duration::days(1)
            }
        };
        let first = match self.file_repository.max_period(granularity, station_id)? {
            Some(max) => next_period_date(&max, granularity)?,
            None => NaiveDate::from_ymd_opt(EXPORT_EPOCH.0, EXPORT_EPOCH.1, EXPORT_EPOCH.2)
                .ok_or_else(|| {
                    DomainError::InvalidQuery("invalid export epoch date".to_string())
                })?,
        };
        Ok((first, latest_complete))
    }

    /// Publishes one period's three distribution files and returns how many
    /// were actually appended (skipping periods that have no measurements and
    /// files that already exist).
    fn export_period(
        &self,
        job_id: Uuid,
        granularity: Granularity,
        period: String,
        station_id: Option<Uuid>,
    ) -> Result<usize, DomainError> {
        let tz = Self::timezone()?;
        let (from_utc, to_utc) = period_window(tz, granularity, &period)?;
        let rows = self.measurement_reader.rows(from_utc, to_utc, station_id)?;
        if rows.is_empty() {
            return Ok(0);
        }

        let mut added = 0usize;
        for format in Format::ALL {
            // Cancellation is honored after each file.
            self.check_cancellation(job_id)?;
            let key = object_key(station_id, granularity, &period, format);
            if self.file_repository.find_by_object_key(&key)?.is_some() {
                continue; // already published, immutable
            }
            let bytes = self.generator.generate(&rows, format)?;
            let sha256 = sha256_hex(&bytes);
            self.storage.put(
                &ObjectKey(key.clone()),
                &ContentType(format.content_type().to_string()),
                &bytes,
            )?;
            self.file_repository.insert(&OpenDataFile {
                id: Uuid::new_v4(),
                object_key: key,
                station_id,
                granularity,
                period: period.clone(),
                format,
                byte_size: bytes.len() as i64,
                sha256,
                created_at: Utc::now(),
            })?;
            added += 1;
        }
        Ok(added)
    }

    fn timezone() -> Result<Tz, DomainError> {
        OPENDATA_TIMEZONE.parse::<Tz>().map_err(|_| {
            DomainError::InvalidQuery(format!("unknown IANA timezone '{OPENDATA_TIMEZONE}'"))
        })
    }

    /// Heartbeats the job and returns [`DomainError::Cancelled`] when a
    /// cancellation was requested, so the export loop stops gracefully.
    fn check_cancellation(&self, job_id: Uuid) -> Result<(), DomainError> {
        let now = Utc::now();
        let interval = self.configuration.opendata_export_max_heartbeat_interval();
        match self.job_repository.heartbeat(
            job_id,
            OPENDATA_EXPORT_JOB_TYPE,
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

impl ScheduledJobPort for OpenDataExportService {
    fn run_if_due(&self) {
        self.run_if_due();
    }
}

/// The local midnight of `date` in `tz` as a UTC instant.
fn local_midnight_utc(tz: Tz, date: NaiveDate) -> Result<DateTime<Utc>, DomainError> {
    let naive = date
        .and_hms_opt(0, 0, 0)
        .ok_or_else(|| DomainError::InvalidQuery("invalid date".to_string()))?;
    tz.from_local_datetime(&naive)
        .earliest()
        .map(|local| local.with_timezone(&Utc))
        .ok_or_else(|| {
            DomainError::InvalidQuery(format!("local midnight does not exist in timezone {tz}"))
        })
}

/// The first date of the period following `period` (daily: next day; monthly:
/// first of next month).
fn next_period_date(period: &str, granularity: Granularity) -> Result<NaiveDate, DomainError> {
    let date = period_first_date(period, granularity)?;
    Ok(match granularity {
        Granularity::Daily => date + Duration::days(1),
        Granularity::Monthly => {
            let (year, month) = (date.year(), date.month());
            if month == 12 {
                NaiveDate::from_ymd_opt(year + 1, 1, 1)
            } else {
                NaiveDate::from_ymd_opt(year, month + 1, 1)
            }
            .ok_or_else(|| DomainError::InvalidQuery("invalid next month".to_string()))?
        }
    })
}

/// The first calendar date of a period (`YYYY-MM-DD` or `YYYY-MM`).
fn period_first_date(period: &str, granularity: Granularity) -> Result<NaiveDate, DomainError> {
    let invalid = || {
        DomainError::InvalidQuery(format!(
            "invalid {} period '{period}'",
            granularity.as_str()
        ))
    };
    match granularity {
        Granularity::Daily => NaiveDate::parse_from_str(period, "%Y-%m-%d").map_err(|_| invalid()),
        Granularity::Monthly => {
            let (year, month) = period.split_once('-').ok_or_else(invalid)?;
            let year: i32 = year.parse().map_err(|_| invalid())?;
            let month: u32 = month.parse().map_err(|_| invalid())?;
            NaiveDate::from_ymd_opt(year, month, 1).ok_or_else(invalid)
        }
    }
}

/// The closed UTC window of one period (`[from, to]`, inclusive upper bound).
fn period_window(
    tz: Tz,
    granularity: Granularity,
    period: &str,
) -> Result<(DateTime<Utc>, DateTime<Utc>), DomainError> {
    let first = period_first_date(period, granularity)?;
    let next = match granularity {
        Granularity::Daily => first + Duration::days(1),
        Granularity::Monthly => {
            let (year, month) = (first.year(), first.month());
            if month == 12 {
                NaiveDate::from_ymd_opt(year + 1, 1, 1)
                    .ok_or_else(|| DomainError::InvalidQuery("invalid next month".to_string()))?
            } else {
                NaiveDate::from_ymd_opt(year, month + 1, 1)
                    .ok_or_else(|| DomainError::InvalidQuery("invalid next month".to_string()))?
            }
        }
    };
    let from_utc = local_midnight_utc(tz, first)?;
    let to_utc = local_midnight_utc(tz, next)? - Duration::microseconds(1);
    Ok((from_utc, to_utc))
}

/// Lowercase hex SHA-256 of `bytes`.
fn sha256_hex(bytes: &[u8]) -> String {
    Sha2Digest::digest(bytes)
        .iter()
        .map(|byte| format!("{byte:02x}"))
        .collect()
}

#[cfg(test)]
mod tests {
    use std::collections::HashMap;
    use std::future::Future;
    use std::pin::Pin;
    use std::sync::atomic::{AtomicUsize, Ordering};
    use std::sync::{Arc, Mutex};

    use futures::Stream;

    use super::*;
    use crate::core::domain::assets::asset_storage_port::{AssetObjectInfo, AssetObjectStream};
    use crate::core::domain::configuration::configuration::value_objects::{
        AssetStorageConfiguration, DatabaseConfiguration, MapsConfiguration,
    };
    use crate::core::domain::configuration::configuration::{
        DEFAULT_ASSET_CLEANUP_CRON, DEFAULT_DATA_SOURCE_UPDATE_CRON,
    };
    use crate::core::domain::counting_stations::counting_station::CountingStation;
    use crate::core::domain::counting_stations::counting_station::value_objects as station_vo;
    use crate::core::domain::opendata::measurement::OpenDataMeasurement;

    const INSTANCE: Uuid = Uuid::from_u128(0x0000_0000_0000_0000_0000_0000_0000_00E1);

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

    // -- In-memory ports ------------------------------------------------------

    struct MemoryJobRepository {
        jobs: Mutex<Vec<Job>>,
        locks: Mutex<HashMap<String, (Uuid, DateTime<Utc>)>>,
        // Test-only error-injection toggles so the export service's failure
        // branches can be exercised without a real database.
        fail_find_active: bool,
        fail_find_last_finished: bool,
        fail_acquire: bool,
        fail_insert: bool,
        fail_mark_cancelled: bool,
        fail_set_finished: bool,
        fail_set_failed: bool,
    }

    impl MemoryJobRepository {
        fn new(jobs: Vec<Job>) -> Self {
            Self {
                jobs: Mutex::new(jobs),
                locks: Mutex::new(HashMap::new()),
                fail_find_active: false,
                fail_find_last_finished: false,
                fail_acquire: false,
                fail_insert: false,
                fail_mark_cancelled: false,
                fail_set_finished: false,
                fail_set_failed: false,
            }
        }
    }

    impl JobRepository for MemoryJobRepository {
        fn insert(&self, job: Job) -> Result<(), DomainError> {
            if self.fail_insert {
                return Err(DomainError::Database("insert boom".to_string()));
            }
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
                return Err(DomainError::Database("acquire boom".to_string()));
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
            if self.fail_set_finished {
                return Err(DomainError::Database("set_finished boom".to_string()));
            }
            let mut jobs = self.jobs.lock().unwrap();
            let job = jobs
                .iter_mut()
                .find(|job| job.id == id)
                .ok_or(DomainError::NotFound(id))?;
            if job.status != JobStatus::Running {
                return Err(DomainError::InvalidQuery("not running".to_string()));
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
            if self.fail_set_failed {
                return Err(DomainError::Database("set_failed boom".to_string()));
            }
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
            if self.fail_mark_cancelled {
                return Err(DomainError::Database("mark_cancelled boom".to_string()));
            }
            let mut jobs = self.jobs.lock().unwrap();
            let job = jobs
                .iter_mut()
                .find(|job| job.id == id)
                .ok_or(DomainError::NotFound(id))?;
            if !matches!(
                job.status,
                JobStatus::Running | JobStatus::CancellationRequested
            ) {
                return Err(DomainError::InvalidQuery("not cancellable".to_string()));
            }
            job.status = JobStatus::Cancelled;
            job.finished_at = Some(finished_at);
            job.failure_message = Some("cancelled".to_string());
            Ok(())
        }
        fn update_metadata(
            &self,
            id: Uuid,
            key: &str,
            value: serde_json::Value,
        ) -> Result<(), DomainError> {
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
                return Err(DomainError::Database("find_active boom".to_string()));
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
                return Err(DomainError::Database("find_last_finished boom".to_string()));
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
        fn reconcile_stale_active(
            &self,
            _job_type: &str,
            _heartbeat_before: DateTime<Utc>,
            _now: DateTime<Utc>,
        ) -> Result<(), DomainError> {
            Ok(())
        }
    }

    struct MemoryOpenDataFileRepository {
        files: Mutex<Vec<OpenDataFile>>,
    }

    impl MemoryOpenDataFileRepository {
        fn new(files: Vec<OpenDataFile>) -> Self {
            Self {
                files: Mutex::new(files),
            }
        }
    }

    impl OpenDataFileRepository for MemoryOpenDataFileRepository {
        fn insert(&self, file: &OpenDataFile) -> Result<(), DomainError> {
            if self.find_by_object_key(&file.object_key)?.is_some() {
                return Err(DomainError::InvalidQuery(format!(
                    "duplicate object key {}",
                    file.object_key
                )));
            }
            self.files.lock().unwrap().push(file.clone());
            Ok(())
        }
        fn find_by_object_key(
            &self,
            object_key: &str,
        ) -> Result<Option<OpenDataFile>, DomainError> {
            Ok(self
                .files
                .lock()
                .unwrap()
                .iter()
                .find(|file| file.object_key == object_key)
                .cloned())
        }
        fn list_periods(
            &self,
            granularity: Granularity,
            station_id: Option<Uuid>,
        ) -> Result<Vec<String>, DomainError> {
            let mut periods: Vec<String> = self
                .files
                .lock()
                .unwrap()
                .iter()
                .filter(|file| file.granularity == granularity && file.station_id == station_id)
                .map(|file| file.period.clone())
                .collect();
            periods.sort_by(|a, b| b.cmp(a));
            periods.dedup();
            Ok(periods)
        }
        fn find_by_period(
            &self,
            granularity: Granularity,
            period: &str,
            station_id: Option<Uuid>,
        ) -> Result<Vec<OpenDataFile>, DomainError> {
            Ok(self
                .files
                .lock()
                .unwrap()
                .iter()
                .filter(|file| {
                    file.granularity == granularity
                        && file.period == period
                        && file.station_id == station_id
                })
                .cloned()
                .collect())
        }
        fn max_period(
            &self,
            granularity: Granularity,
            station_id: Option<Uuid>,
        ) -> Result<Option<String>, DomainError> {
            Ok(self
                .list_periods(granularity, station_id)?
                .into_iter()
                .next())
        }
    }

    struct StubMeasurementReader {
        daily: Vec<String>,
        monthly: Vec<String>,
        rows: Vec<OpenDataMeasurement>,
    }

    impl OpenDataMeasurementReader for StubMeasurementReader {
        fn rows(
            &self,
            _from: DateTime<Utc>,
            _to: DateTime<Utc>,
            _station_id: Option<Uuid>,
        ) -> Result<Vec<OpenDataMeasurement>, DomainError> {
            Ok(self.rows.clone())
        }
        fn available_periods(
            &self,
            granularity: Granularity,
            _from: DateTime<Utc>,
            _to: DateTime<Utc>,
            _station_id: Option<Uuid>,
        ) -> Result<Vec<String>, DomainError> {
            Ok(match granularity {
                Granularity::Daily => self.daily.clone(),
                Granularity::Monthly => self.monthly.clone(),
            })
        }
    }

    struct StubGenerator {
        fail: bool,
        calls: AtomicUsize,
    }

    impl OpenDataFileGenerator for StubGenerator {
        fn generate(
            &self,
            _rows: &[OpenDataMeasurement],
            format: Format,
        ) -> Result<Vec<u8>, DomainError> {
            if self.fail {
                return Err(DomainError::Database("generate boom".to_string()));
            }
            self.calls.fetch_add(1, Ordering::SeqCst);
            Ok(format!("content:{}", format.as_str()).into_bytes())
        }
    }

    struct MemoryStorage {
        objects: Mutex<HashMap<String, Vec<u8>>>,
    }

    impl AssetStorage for MemoryStorage {
        fn ensure_bucket(&self) -> Result<(), DomainError> {
            Ok(())
        }
        fn put(
            &self,
            object_key: &ObjectKey,
            _content_type: &ContentType,
            bytes: &[u8],
        ) -> Result<AssetObjectInfo, DomainError> {
            self.objects
                .lock()
                .unwrap()
                .insert(object_key.0.clone(), bytes.to_vec());
            Ok(AssetObjectInfo {
                byte_size: bytes.len() as i64,
            })
        }
        fn list_object_keys(&self) -> Result<Vec<ObjectKey>, DomainError> {
            Ok(self
                .objects
                .lock()
                .unwrap()
                .keys()
                .map(|key| ObjectKey(key.clone()))
                .collect())
        }
        fn delete(&self, _object_key: &ObjectKey) -> Result<(), DomainError> {
            Ok(())
        }
        fn get_stream(
            &self,
            _object_key: &ObjectKey,
        ) -> Pin<Box<dyn Future<Output = Result<AssetObjectStream, DomainError>> + Send + '_>>
        {
            Box::pin(async {
                let body: Box<
                    dyn Stream<Item = Result<bytes::Bytes, std::io::Error>> + Send + Unpin,
                > = Box::new(futures::stream::iter(Vec::<
                    Result<bytes::Bytes, std::io::Error>,
                >::new()));
                Ok(AssetObjectStream { body })
            })
        }
    }

    fn active_station(id: Uuid) -> CountingStation {
        CountingStation {
            id: station_vo::Id(id),
            name: station_vo::Name("Station".to_string()),
            description: station_vo::Description("desc".to_string()),
            external_datasource_id: None,
            data_source_id: None,
            coordinates: None,
            timezone: station_vo::Timezone(OPENDATA_TIMEZONE.to_string()),
            image_asset_id: None,
            image_sha256: None,
            status: Status::Active,
        }
    }

    struct MemoryCountingStationRepository {
        stations: Vec<CountingStation>,
    }

    impl CountingStationRepository for MemoryCountingStationRepository {
        fn save(&self, _station: CountingStation) -> Result<(), DomainError> {
            Ok(())
        }
        fn find_by_id(&self, id: station_vo::Id) -> Result<CountingStation, DomainError> {
            self.stations
                .iter()
                .find(|s| s.id == id)
                .cloned()
                .ok_or(DomainError::NotFound(id.0))
        }
        fn find_all(&self) -> Result<Vec<CountingStation>, DomainError> {
            Ok(self.stations.clone())
        }
        fn find_by_external_datasource_id(
            &self,
            _external_id: station_vo::ExternalDatasourceId,
        ) -> Result<Option<CountingStation>, DomainError> {
            Ok(None)
        }
        fn find_filtered(&self, _name: Option<&str>) -> Result<Vec<CountingStation>, DomainError> {
            Ok(self.stations.clone())
        }
        fn update(&self, _station: CountingStation) -> Result<(), DomainError> {
            Ok(())
        }
    }

    fn running_job() -> Job {
        Job::running(
            Uuid::new_v4(),
            OPENDATA_EXPORT_JOB_NAME.to_string(),
            OPENDATA_EXPORT_JOB_TYPE.to_string(),
            INSTANCE,
            Utc::now(),
        )
    }

    fn measurement_row() -> OpenDataMeasurement {
        OpenDataMeasurement {
            station_id: Uuid::from_u128(1),
            channel_id: Uuid::from_u128(2),
            channel_name: "Channel A".to_string(),
            timestamp: NaiveDate::from_ymd_opt(2026, 9, 5)
                .unwrap()
                .and_hms_opt(12, 0, 0)
                .unwrap(),
            value: 42,
            resolution_seconds: 3600,
        }
    }

    /// Yesterday's Berlin date string (the daily export window is time-relative).
    fn yesterday() -> String {
        let tz = OPENDATA_TIMEZONE.parse::<Tz>().unwrap();
        let today = Utc::now().with_timezone(&tz).date_naive();
        (today - Duration::days(1)).format("%Y-%m-%d").to_string()
    }

    /// The previous calendar month's `YYYY-MM`.
    fn previous_month() -> String {
        let tz = OPENDATA_TIMEZONE.parse::<Tz>().unwrap();
        let today = Utc::now().with_timezone(&tz).date_naive();
        let first = today.with_day(1).unwrap();
        (first - Duration::days(1)).format("%Y-%m").to_string()
    }

    fn service(
        file_repo: Arc<MemoryOpenDataFileRepository>,
        reader: Arc<StubMeasurementReader>,
        generator: Arc<StubGenerator>,
        storage: Arc<MemoryStorage>,
        stations: Vec<CountingStation>,
        job_repo: Arc<MemoryJobRepository>,
    ) -> OpenDataExportService {
        OpenDataExportService::new(
            job_repo,
            file_repo,
            reader,
            generator,
            storage,
            Arc::new(MemoryCountingStationRepository { stations }),
            configuration(),
            INSTANCE,
        )
    }

    fn insert_running_job(job_repo: &MemoryJobRepository) -> Uuid {
        job_repo.insert(running_job()).unwrap();
        job_repo.jobs.lock().unwrap()[0].id
    }

    /// A service whose in-memory ports are all empty (no data, no stations, no
    /// registered files) - used by the scheduling/finalization error tests.
    fn noop_service(job_repo: Arc<MemoryJobRepository>) -> OpenDataExportService {
        service(
            Arc::new(MemoryOpenDataFileRepository::new(Vec::new())),
            Arc::new(StubMeasurementReader {
                daily: Vec::new(),
                monthly: Vec::new(),
                rows: Vec::new(),
            }),
            Arc::new(StubGenerator {
                fail: false,
                calls: AtomicUsize::new(0),
            }),
            Arc::new(MemoryStorage {
                objects: Mutex::new(HashMap::new()),
            }),
            Vec::new(),
            job_repo,
        )
    }

    /// A Berlin-local date `days` days before today (`YYYY-MM-DD`).
    fn days_ago(days: i64) -> String {
        let tz = OPENDATA_TIMEZONE.parse::<Tz>().unwrap();
        let today = Utc::now().with_timezone(&tz).date_naive();
        (today - Duration::days(days))
            .format("%Y-%m-%d")
            .to_string()
    }

    #[test]
    fn exports_global_and_station_files_for_complete_periods() {
        let day = yesterday();
        let month = previous_month();
        let station = Uuid::from_u128(1);
        let job_repo = Arc::new(MemoryJobRepository::new(Vec::new()));
        let job_id = insert_running_job(&job_repo);
        let file_repo = Arc::new(MemoryOpenDataFileRepository::new(Vec::new()));
        let reader = Arc::new(StubMeasurementReader {
            daily: vec![day.clone()],
            monthly: vec![month.clone()],
            rows: vec![measurement_row()],
        });
        let generator = Arc::new(StubGenerator {
            fail: false,
            calls: AtomicUsize::new(0),
        });
        let storage = Arc::new(MemoryStorage {
            objects: Mutex::new(HashMap::new()),
        });
        let svc = service(
            file_repo.clone(),
            reader,
            generator,
            storage.clone(),
            vec![active_station(station)],
            job_repo.clone(),
        );

        let added = svc.run_export(job_id).unwrap();

        // 4 scopes (global daily/monthly + station daily/monthly) x 3 formats.
        assert_eq!(added, 12);
        // The job-info metadata reports the files created by this run.
        let job = job_repo.find_by_id(job_id).unwrap().unwrap();
        assert_eq!(job.metadata[FILES_CREATED_KEY], json!(12));
        assert_eq!(storage.list_object_keys().unwrap().len(), 12);
        let files = file_repo.files.lock().unwrap();
        assert_eq!(files.len(), 12);
        assert!(
            files.iter().any(
                |f| f.object_key == object_key(None, Granularity::Daily, &day, Format::Parquet)
            )
        );
        assert!(
            files
                .iter()
                .any(|f| f.object_key
                    == object_key(None, Granularity::Monthly, &month, Format::Json))
        );
        assert!(files.iter().any(|f| f.object_key
            == object_key(Some(station), Granularity::Monthly, &month, Format::CsvGz)));
        assert!(files.iter().all(|f| f.sha256.len() == 64));
    }

    #[test]
    fn skips_periods_without_measurements() {
        let job_repo = Arc::new(MemoryJobRepository::new(Vec::new()));
        let job_id = insert_running_job(&job_repo);
        let reader = Arc::new(StubMeasurementReader {
            daily: Vec::new(),
            monthly: Vec::new(),
            rows: Vec::new(),
        });
        let generator = Arc::new(StubGenerator {
            fail: false,
            calls: AtomicUsize::new(0),
        });
        let svc = service(
            Arc::new(MemoryOpenDataFileRepository::new(Vec::new())),
            reader,
            generator.clone(),
            Arc::new(MemoryStorage {
                objects: Mutex::new(HashMap::new()),
            }),
            vec![active_station(Uuid::from_u128(1))],
            job_repo,
        );

        let added = svc
            .run_export(job_id)
            .expect("an empty export is a successful no-op");
        assert_eq!(added, 0);
        assert_eq!(generator.calls.load(Ordering::SeqCst), 0);
    }

    #[test]
    fn does_not_rewrite_already_registered_files() {
        let day = yesterday();
        let month = previous_month();
        let job_repo = Arc::new(MemoryJobRepository::new(Vec::new()));
        let job_id = insert_running_job(&job_repo);
        // One of the twelve files already exists -> only 11 are appended.
        let existing_key = object_key(None, Granularity::Daily, &day, Format::Json);
        let seeded = OpenDataFile {
            id: Uuid::new_v4(),
            object_key: existing_key.clone(),
            station_id: None,
            granularity: Granularity::Daily,
            period: day.clone(),
            format: Format::Json,
            byte_size: 1,
            sha256: "a".repeat(64),
            created_at: Utc::now(),
        };
        let file_repo = Arc::new(MemoryOpenDataFileRepository::new(vec![seeded]));
        let reader = Arc::new(StubMeasurementReader {
            daily: vec![day.clone()],
            monthly: vec![month.clone()],
            rows: vec![measurement_row()],
        });
        let svc = service(
            file_repo,
            reader,
            Arc::new(StubGenerator {
                fail: false,
                calls: AtomicUsize::new(0),
            }),
            Arc::new(MemoryStorage {
                objects: Mutex::new(HashMap::new()),
            }),
            vec![active_station(Uuid::from_u128(1))],
            job_repo,
        );

        let added = svc.run_export(job_id).unwrap();
        assert_eq!(added, 11);
        let _ = existing_key;
    }

    #[test]
    fn stops_on_cancellation_request() {
        let job_repo = Arc::new(MemoryJobRepository::new(Vec::new()));
        let job_id = insert_running_job(&job_repo);
        job_repo.request_cancellation(job_id).unwrap();

        let svc = service(
            Arc::new(MemoryOpenDataFileRepository::new(Vec::new())),
            Arc::new(StubMeasurementReader {
                daily: vec![yesterday()],
                monthly: vec![previous_month()],
                rows: vec![measurement_row()],
            }),
            Arc::new(StubGenerator {
                fail: false,
                calls: AtomicUsize::new(0),
            }),
            Arc::new(MemoryStorage {
                objects: Mutex::new(HashMap::new()),
            }),
            vec![active_station(Uuid::from_u128(1))],
            job_repo,
        );

        let result = svc.run_export(job_id);
        assert!(matches!(result, Err(DomainError::Cancelled)));
    }

    #[test]
    fn finalizes_failed_when_generation_errors() {
        let job_repo = Arc::new(MemoryJobRepository::new(Vec::new()));
        let job_id = insert_running_job(&job_repo);
        let svc = service(
            Arc::new(MemoryOpenDataFileRepository::new(Vec::new())),
            Arc::new(StubMeasurementReader {
                daily: vec![yesterday()],
                monthly: Vec::new(),
                rows: vec![measurement_row()],
            }),
            Arc::new(StubGenerator {
                fail: true,
                calls: AtomicUsize::new(0),
            }),
            Arc::new(MemoryStorage {
                objects: Mutex::new(HashMap::new()),
            }),
            Vec::new(),
            job_repo.clone(),
        );

        let error = svc.run_export(job_id).expect_err("must fail");
        svc.finalize(job_id, OPENDATA_EXPORT_JOB_NAME, Err(error));
        let stored = job_repo.find_by_id(job_id).unwrap().unwrap();
        assert_eq!(stored.status, JobStatus::Failed);
        assert!(stored.failure_message.is_some());
    }

    #[test]
    fn finalizes_cancelled_when_cancellation_was_requested() {
        let job_repo = Arc::new(MemoryJobRepository::new(Vec::new()));
        let job_id = insert_running_job(&job_repo);
        job_repo.request_cancellation(job_id).unwrap();
        let svc = service(
            Arc::new(MemoryOpenDataFileRepository::new(Vec::new())),
            Arc::new(StubMeasurementReader {
                daily: Vec::new(),
                monthly: Vec::new(),
                rows: Vec::new(),
            }),
            Arc::new(StubGenerator {
                fail: false,
                calls: AtomicUsize::new(0),
            }),
            Arc::new(MemoryStorage {
                objects: Mutex::new(HashMap::new()),
            }),
            Vec::new(),
            job_repo.clone(),
        );

        svc.finalize(job_id, OPENDATA_EXPORT_JOB_NAME, Ok(0));
        let stored = job_repo.find_by_id(job_id).unwrap().unwrap();
        assert_eq!(stored.status, JobStatus::Cancelled);
    }

    #[test]
    fn skips_while_another_run_is_active() {
        let job_repo = Arc::new(MemoryJobRepository::new(vec![running_job()]));
        let svc = service(
            Arc::new(MemoryOpenDataFileRepository::new(Vec::new())),
            Arc::new(StubMeasurementReader {
                daily: Vec::new(),
                monthly: Vec::new(),
                rows: Vec::new(),
            }),
            Arc::new(StubGenerator {
                fail: false,
                calls: AtomicUsize::new(0),
            }),
            Arc::new(MemoryStorage {
                objects: Mutex::new(HashMap::new()),
            }),
            Vec::new(),
            job_repo.clone(),
        );

        svc.run_if_due();
        assert_eq!(job_repo.jobs.lock().unwrap().len(), 1);
    }

    #[test]
    fn runs_and_finishes_when_it_has_never_succeeded() {
        let job_repo = Arc::new(MemoryJobRepository::new(Vec::new()));
        let svc = service(
            Arc::new(MemoryOpenDataFileRepository::new(Vec::new())),
            Arc::new(StubMeasurementReader {
                daily: Vec::new(),
                monthly: Vec::new(),
                rows: Vec::new(),
            }),
            Arc::new(StubGenerator {
                fail: false,
                calls: AtomicUsize::new(0),
            }),
            Arc::new(MemoryStorage {
                objects: Mutex::new(HashMap::new()),
            }),
            Vec::new(),
            job_repo.clone(),
        );

        svc.run_if_due();
        let jobs = job_repo.jobs.lock().unwrap();
        let finished = jobs
            .iter()
            .find(|job| job.status == JobStatus::Finished)
            .expect("a finished job must exist");
        assert_eq!(finished.metadata[FILES_CREATED_KEY], json!(0));
    }

    #[test]
    fn does_not_run_when_last_run_is_recent() {
        let mut finished = running_job();
        finished.status = JobStatus::Finished;
        finished.finished_at = Some(Utc::now());
        let job_repo = Arc::new(MemoryJobRepository::new(vec![finished]));
        let svc = service(
            Arc::new(MemoryOpenDataFileRepository::new(Vec::new())),
            Arc::new(StubMeasurementReader {
                daily: Vec::new(),
                monthly: Vec::new(),
                rows: Vec::new(),
            }),
            Arc::new(StubGenerator {
                fail: false,
                calls: AtomicUsize::new(0),
            }),
            Arc::new(MemoryStorage {
                objects: Mutex::new(HashMap::new()),
            }),
            Vec::new(),
            job_repo.clone(),
        );

        svc.run_if_due();
        assert_eq!(
            job_repo
                .jobs
                .lock()
                .unwrap()
                .iter()
                .filter(|job| job.status == JobStatus::Finished)
                .count(),
            1
        );
    }

    #[test]
    fn period_helpers_parse_and_step_dates() {
        let daily = Granularity::Daily;
        let monthly = Granularity::Monthly;
        assert_eq!(
            period_first_date("2026-09-05", daily).unwrap(),
            NaiveDate::from_ymd_opt(2026, 9, 5).unwrap()
        );
        assert_eq!(
            period_first_date("2026-09", monthly).unwrap(),
            NaiveDate::from_ymd_opt(2026, 9, 1).unwrap()
        );
        assert_eq!(
            next_period_date("2026-09-05", daily).unwrap(),
            NaiveDate::from_ymd_opt(2026, 9, 6).unwrap()
        );
        assert_eq!(
            next_period_date("2026-12", monthly).unwrap(),
            NaiveDate::from_ymd_opt(2027, 1, 1).unwrap()
        );
        // A non-December monthly step rolls over into the following month.
        assert_eq!(
            next_period_date("2026-09", monthly).unwrap(),
            NaiveDate::from_ymd_opt(2026, 10, 1).unwrap()
        );
        // A December monthly window spans into January of the next year.
        let tz: Tz = OPENDATA_TIMEZONE.parse().unwrap();
        let (from, to) = period_window(tz, Granularity::Monthly, "2026-12").unwrap();
        assert_eq!(
            from,
            local_midnight_utc(tz, NaiveDate::from_ymd_opt(2026, 12, 1).unwrap()).unwrap()
        );
        assert_eq!(
            to,
            local_midnight_utc(tz, NaiveDate::from_ymd_opt(2027, 1, 1).unwrap()).unwrap()
                - Duration::microseconds(1)
        );
        assert!(period_first_date("nope", daily).is_err());
        assert!(period_first_date("2026-09", daily).is_err());
    }

    #[test]
    fn sha256_hex_produces_a_64_char_digest() {
        assert_eq!(super::sha256_hex(b"content").len(), 64);
    }

    #[test]
    fn runs_when_last_run_is_overdue() {
        let mut last = running_job();
        last.status = JobStatus::Finished;
        last.finished_at = Some(Utc::now() - Duration::days(400));
        let job_repo = Arc::new(MemoryJobRepository::new(vec![last]));

        let svc = noop_service(job_repo.clone());
        svc.run_if_due();

        let jobs = job_repo.jobs.lock().unwrap();
        let finished = jobs
            .iter()
            .filter(|job| job.status == JobStatus::Finished)
            .count();
        assert_eq!(finished, 2);
    }

    #[test]
    fn runs_when_last_finished_job_has_no_anchor_timestamp() {
        let mut last = running_job();
        last.status = JobStatus::Finished;
        last.started_at = None;
        last.finished_at = None;
        let job_repo = Arc::new(MemoryJobRepository::new(vec![last]));

        let svc = noop_service(job_repo.clone());
        svc.run_if_due();

        let jobs = job_repo.jobs.lock().unwrap();
        let finished = jobs
            .iter()
            .filter(|job| job.status == JobStatus::Finished)
            .count();
        assert_eq!(finished, 2);
    }

    #[test]
    fn run_if_due_reports_when_active_check_fails() {
        let mut repo = MemoryJobRepository::new(Vec::new());
        repo.fail_find_active = true;
        let job_repo = Arc::new(repo);

        let svc = noop_service(job_repo.clone());
        svc.run_if_due();
        assert_eq!(job_repo.jobs.lock().unwrap().len(), 0);
    }

    #[test]
    fn run_if_due_reports_when_last_finished_check_fails() {
        let mut repo = MemoryJobRepository::new(Vec::new());
        repo.fail_find_last_finished = true;
        let job_repo = Arc::new(repo);

        let svc = noop_service(job_repo.clone());
        svc.run_if_due();
        assert_eq!(job_repo.jobs.lock().unwrap().len(), 0);
    }

    #[test]
    fn execute_skips_when_the_lock_is_held_elsewhere() {
        let job_repo = Arc::new(MemoryJobRepository::new(Vec::new()));
        job_repo.locks.lock().unwrap().insert(
            OPENDATA_EXPORT_JOB_TYPE.to_string(),
            (Uuid::new_v4(), Utc::now() + Duration::hours(1)),
        );

        let svc = noop_service(job_repo.clone());
        svc.execute(Utc::now());
        assert_eq!(job_repo.jobs.lock().unwrap().len(), 0);
    }

    #[test]
    fn execute_stops_when_acquiring_the_lock_fails() {
        let mut repo = MemoryJobRepository::new(Vec::new());
        repo.fail_acquire = true;
        let job_repo = Arc::new(repo);

        let svc = noop_service(job_repo.clone());
        svc.execute(Utc::now());
        assert_eq!(job_repo.jobs.lock().unwrap().len(), 0);
    }

    #[test]
    fn execute_stops_when_recording_the_job_fails() {
        let mut repo = MemoryJobRepository::new(Vec::new());
        repo.fail_insert = true;
        let job_repo = Arc::new(repo);

        let svc = noop_service(job_repo.clone());
        svc.execute(Utc::now());
        assert_eq!(job_repo.jobs.lock().unwrap().len(), 0);
        assert!(
            !job_repo
                .locks
                .lock()
                .unwrap()
                .contains_key(OPENDATA_EXPORT_JOB_TYPE)
        );
    }

    #[test]
    fn finalize_reports_when_mark_cancelled_fails() {
        let mut repo = MemoryJobRepository::new(Vec::new());
        let job_id = insert_running_job(&repo);
        repo.request_cancellation(job_id).unwrap();
        repo.fail_mark_cancelled = true;
        let job_repo = Arc::new(repo);

        let svc = noop_service(job_repo.clone());
        svc.finalize(job_id, OPENDATA_EXPORT_JOB_NAME, Ok(0));

        let stored = job_repo.find_by_id(job_id).unwrap().unwrap();
        assert_eq!(stored.status, JobStatus::CancellationRequested);
    }

    #[test]
    fn finalize_reports_when_set_finished_fails() {
        let mut repo = MemoryJobRepository::new(Vec::new());
        let job_id = insert_running_job(&repo);
        repo.fail_set_finished = true;
        let job_repo = Arc::new(repo);

        let svc = noop_service(job_repo.clone());
        svc.finalize(job_id, OPENDATA_EXPORT_JOB_NAME, Ok(3));

        let stored = job_repo.find_by_id(job_id).unwrap().unwrap();
        assert_eq!(stored.status, JobStatus::Running);
        assert_eq!(stored.metadata[FILES_CREATED_KEY], json!(3));
    }

    #[test]
    fn finalize_reports_when_set_failed_fails() {
        let mut repo = MemoryJobRepository::new(Vec::new());
        let job_id = insert_running_job(&repo);
        repo.fail_set_failed = true;
        let job_repo = Arc::new(repo);

        let svc = noop_service(job_repo.clone());
        svc.finalize(
            job_id,
            OPENDATA_EXPORT_JOB_NAME,
            Err(DomainError::Database("boom".to_string())),
        );

        let stored = job_repo.find_by_id(job_id).unwrap().unwrap();
        assert_eq!(stored.status, JobStatus::Running);
    }

    #[test]
    fn skips_already_registered_periods_before_exporting_newer() {
        let day = yesterday();
        let older = days_ago(2);
        let job_repo = Arc::new(MemoryJobRepository::new(Vec::new()));
        let job_id = insert_running_job(&job_repo);
        // A fully published period older than yesterday forces the available
        // window loop to hit the "already registered" `continue` branch.
        let seeded = OpenDataFile {
            id: Uuid::new_v4(),
            object_key: object_key(None, Granularity::Daily, &older, Format::Parquet),
            station_id: None,
            granularity: Granularity::Daily,
            period: older.clone(),
            format: Format::Parquet,
            byte_size: 1,
            sha256: "a".repeat(64),
            created_at: Utc::now(),
        };
        let file_repo = Arc::new(MemoryOpenDataFileRepository::new(vec![seeded]));
        let reader = Arc::new(StubMeasurementReader {
            daily: vec![older, day.clone()],
            monthly: Vec::new(),
            rows: vec![measurement_row()],
        });

        let svc = service(
            file_repo.clone(),
            reader,
            Arc::new(StubGenerator {
                fail: false,
                calls: AtomicUsize::new(0),
            }),
            Arc::new(MemoryStorage {
                objects: Mutex::new(HashMap::new()),
            }),
            Vec::new(),
            job_repo,
        );

        // Only yesterday's three formats are new; the older period is skipped.
        let added = svc.run_export(job_id).unwrap();
        assert_eq!(added, 3);
        assert_eq!(file_repo.files.lock().unwrap().len(), 4);
        assert!(
            file_repo
                .files
                .lock()
                .unwrap()
                .iter()
                .any(|f| f.object_key == object_key(None, Granularity::Daily, &day, Format::Json))
        );
    }

    #[test]
    fn complete_newest_period_ignores_a_future_registry_period() {
        let future = "2099-12-31".to_string();
        let job_repo = Arc::new(MemoryJobRepository::new(Vec::new()));
        let job_id = insert_running_job(&job_repo);
        let seeded = OpenDataFile {
            id: Uuid::new_v4(),
            object_key: object_key(None, Granularity::Daily, &future, Format::Parquet),
            station_id: None,
            granularity: Granularity::Daily,
            period: future.clone(),
            format: Format::Parquet,
            byte_size: 1,
            sha256: "a".repeat(64),
            created_at: Utc::now(),
        };
        let file_repo = Arc::new(MemoryOpenDataFileRepository::new(vec![seeded]));

        let svc = service(
            file_repo.clone(),
            Arc::new(StubMeasurementReader {
                daily: Vec::new(),
                monthly: Vec::new(),
                rows: vec![measurement_row()],
            }),
            Arc::new(StubGenerator {
                fail: false,
                calls: AtomicUsize::new(0),
            }),
            Arc::new(MemoryStorage {
                objects: Mutex::new(HashMap::new()),
            }),
            Vec::new(),
            job_repo,
        );

        let added = svc.run_export(job_id).unwrap();
        assert_eq!(added, 0);
        assert_eq!(file_repo.files.lock().unwrap().len(), 1);
    }

    #[test]
    fn export_period_skips_periods_that_return_no_rows() {
        let job_repo = Arc::new(MemoryJobRepository::new(Vec::new()));
        let job_id = insert_running_job(&job_repo);
        let generator = Arc::new(StubGenerator {
            fail: false,
            calls: AtomicUsize::new(0),
        });
        // The reader reports periods as available, but each scope yields no
        // rows, so `export_period` must short-circuit before generating files.
        let reader = Arc::new(StubMeasurementReader {
            daily: vec![yesterday()],
            monthly: vec![previous_month()],
            rows: Vec::new(),
        });

        let svc = service(
            Arc::new(MemoryOpenDataFileRepository::new(Vec::new())),
            reader,
            generator.clone(),
            Arc::new(MemoryStorage {
                objects: Mutex::new(HashMap::new()),
            }),
            Vec::new(),
            job_repo,
        );

        let added = svc.run_export(job_id).unwrap();
        assert_eq!(added, 0);
        assert_eq!(generator.calls.load(Ordering::SeqCst), 0);
    }

    #[test]
    fn check_cancellation_propagates_heartbeat_errors() {
        let job_repo = Arc::new(MemoryJobRepository::new(Vec::new()));
        let svc = noop_service(job_repo);

        let error = svc
            .check_cancellation(Uuid::new_v4())
            .expect_err("a heartbeat for an unknown job must fail");
        assert!(matches!(error, DomainError::NotFound(_)));
    }

    #[test]
    fn scheduled_job_port_delegates_to_the_inherent_runner() {
        let job_repo = Arc::new(MemoryJobRepository::new(vec![running_job()]));
        let svc = noop_service(job_repo.clone());

        ScheduledJobPort::run_if_due(&svc);
        assert_eq!(job_repo.jobs.lock().unwrap().len(), 1);
    }
}
