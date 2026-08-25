//! Application service computing whole-system statistics for the BFF
//! `global-summary` endpoint.
//!
//! This is deliberately separate from the station-summary aggregation: it knows
//! nothing about the map view, bounding boxes or per-station counts, so
//! non-station statistics (last update now, jobs later) can be added without
//! touching the station-summary model.
//!
//! The "last day" total is the sum of every station's previous complete local
//! day total, each computed in the station's own timezone (DST-aware). The
//! low-level measurement sum stays generic (`sum(from, to, channel)`).

use std::collections::HashMap;
use std::sync::Arc;

use chrono::{DateTime, Utc};

use crate::core::application::data_source_update_service::DATA_SOURCE_UPDATE_JOB_TYPE;
use crate::core::domain::channels::repository_port::ChannelRepository;
use crate::core::domain::counting_stations::counting_station::previous_local_day;
use crate::core::domain::counting_stations::repository_port::CountingStationRepository;
use crate::core::domain::error::DomainError;
use crate::core::domain::global_summary::GlobalSummary;
use crate::core::domain::global_summary::service_port::GlobalSummaryServicePort;
use crate::core::domain::jobs::repository_port::JobRepository;
use crate::core::domain::measurements::measurement::value_objects as measurement_vo;
use crate::core::domain::measurements::repository_port::MeasurementRepository;

pub struct GlobalSummaryService {
    counting_station_repository: Arc<dyn CountingStationRepository + Send + Sync>,
    channel_repository: Arc<dyn ChannelRepository + Send + Sync>,
    measurement_repository: Arc<dyn MeasurementRepository + Send + Sync>,
    job_repository: Arc<dyn JobRepository + Send + Sync>,
}

impl GlobalSummaryService {
    pub fn new(
        counting_station_repository: Arc<dyn CountingStationRepository + Send + Sync>,
        channel_repository: Arc<dyn ChannelRepository + Send + Sync>,
        measurement_repository: Arc<dyn MeasurementRepository + Send + Sync>,
        job_repository: Arc<dyn JobRepository + Send + Sync>,
    ) -> Self {
        Self {
            counting_station_repository,
            channel_repository,
            measurement_repository,
            job_repository,
        }
    }

    /// Computes the whole-system statistics for `now`: the sum of every
    /// station's previous complete local day total (each in its own timezone).
    fn compute(&self, now: DateTime<Utc>) -> Result<GlobalSummary, DomainError> {
        let stations = self.counting_station_repository.find_all()?;
        let channels = self.channel_repository.find_all()?;

        let mut channels_by_station: HashMap<uuid::Uuid, Vec<measurement_vo::ChannelId>> =
            HashMap::new();
        for channel in &channels {
            channels_by_station
                .entry(channel.counting_station_id.0)
                .or_default()
                .push(measurement_vo::ChannelId(channel.id.0));
        }

        let mut bikes_last_day_total = 0i64;
        for station in &stations {
            let tz = station.timezone.parse()?;
            let (from, to) = previous_local_day(tz, now)?;
            if let Some(channel_ids) = channels_by_station.get(&station.id.0) {
                for channel_id in channel_ids {
                    bikes_last_day_total +=
                        self.measurement_repository
                            .sum(from, to, Some(*channel_id))?;
                }
            }
        }

        let station_count = stations.len();
        let channel_count = channels.len();
        let last_update = self
            .job_repository
            .find_last_finished_by_type(DATA_SOURCE_UPDATE_JOB_TYPE)?
            .and_then(|job| job.finished_at);

        Ok(GlobalSummary {
            station_count,
            channel_count,
            bikes_last_day_total,
            last_update,
        })
    }
}

impl GlobalSummaryServicePort for GlobalSummaryService {
    fn summarize(&self, now: DateTime<Utc>) -> Result<GlobalSummary, DomainError> {
        self.compute(now)
    }
}

#[cfg(test)]
mod tests {
    use std::sync::Arc;

    use chrono::{TimeZone, Utc};
    use uuid::Uuid;

    use super::*;
    use crate::core::domain::channels::channel::Channel;
    use crate::core::domain::channels::channel::value_objects as channel_vo;
    use crate::core::domain::counting_stations::counting_station::CountingStation;
    use crate::core::domain::counting_stations::counting_station::value_objects as station_vo;
    use crate::core::domain::jobs::job::{Job, JobStatus};
    use crate::core::domain::measurements::measurement::Measurement;
    use crate::core::domain::measurements::measurement::value_objects as measurement_vo;

    fn utc(y: i32, mo: u32, d: u32, h: u32, mi: u32, s: u32) -> DateTime<Utc> {
        Utc.with_ymd_and_hms(y, mo, d, h, mi, s).single().unwrap()
    }

    /// Fixed "now": 2024-01-02 12:00 UTC = 13:00 Berlin (CET, UTC+1), so
    /// yesterday is the Berlin day 2024-01-01 = UTC
    /// `[2023-12-31T23:00:00Z, 2024-01-01T23:00:00Z)`.
    fn now() -> DateTime<Utc> {
        utc(2024, 1, 2, 12, 0, 0)
    }

    fn station(id: u128, name: &str) -> CountingStation {
        CountingStation {
            id: station_vo::Id(Uuid::from_u128(id)),
            name: station_vo::Name(name.to_string()),
            description: station_vo::Description(String::new()),
            external_datasource_id: None,
            data_source_id: None,
            coordinates: None,
            timezone: station_vo::Timezone("Europe/Berlin".to_string()),
            image_asset_id: None,
            image_sha256: None,
        }
    }

    fn channel(id: u128, station_id: u128) -> Channel {
        Channel {
            id: channel_vo::Id(Uuid::from_u128(id)),
            counting_station_id: channel_vo::CountingStationId(Uuid::from_u128(station_id)),
            name: channel_vo::Name(format!("channel-{id}")),
            description: channel_vo::Description(String::new()),
            external_datasource_id: None,
        }
    }

    fn measurement(id: u128, channel_id: u128, value: i64, when: DateTime<Utc>) -> Measurement {
        Measurement {
            id: measurement_vo::Id(Uuid::from_u128(id)),
            value: measurement_vo::Value(value),
            channel_id: measurement_vo::ChannelId(Uuid::from_u128(channel_id)),
            timestamp: measurement_vo::Timestamp(when),
        }
    }

    fn finished_job(id: u128, finished_at: DateTime<Utc>) -> Job {
        let mut job = Job::new(
            Uuid::from_u128(id),
            "Data source update".to_string(),
            DATA_SOURCE_UPDATE_JOB_TYPE.to_string(),
            finished_at + chrono::Duration::hours(1),
        );
        job.status = JobStatus::Finished;
        job.started_at = Some(finished_at - chrono::Duration::minutes(5));
        job.finished_at = Some(finished_at);
        job
    }

    struct MemoryCountingStationRepository {
        stations: Vec<CountingStation>,
    }

    impl CountingStationRepository for MemoryCountingStationRepository {
        fn save(&self, _station: CountingStation) -> Result<(), DomainError> {
            Ok(())
        }
        fn update(&self, _station: CountingStation) -> Result<(), DomainError> {
            Ok(())
        }
        fn find_by_id(&self, _id: station_vo::Id) -> Result<CountingStation, DomainError> {
            Err(DomainError::NotFound(Uuid::new_v4()))
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
    }

    struct MemoryChannelRepository {
        channels: Vec<Channel>,
    }

    impl ChannelRepository for MemoryChannelRepository {
        fn save(&self, _channel: Channel) -> Result<(), DomainError> {
            Ok(())
        }
        fn find_by_id(&self, _id: channel_vo::Id) -> Result<Channel, DomainError> {
            Err(DomainError::NotFound(Uuid::new_v4()))
        }
        fn find_all(&self) -> Result<Vec<Channel>, DomainError> {
            Ok(self.channels.clone())
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
            Ok(self.channels.clone())
        }
    }

    struct MemoryMeasurementRepository {
        measurements: Vec<Measurement>,
    }

    impl MeasurementRepository for MemoryMeasurementRepository {
        fn save(&self, _measurement: Measurement) -> Result<(), DomainError> {
            Ok(())
        }
        fn save_batch(&self, _measurements: Vec<Measurement>) -> Result<u64, DomainError> {
            Ok(0)
        }
        fn find_by_id(&self, _id: measurement_vo::Id) -> Result<Measurement, DomainError> {
            Err(DomainError::NotFound(Uuid::new_v4()))
        }
        fn find_all(&self) -> Result<Vec<Measurement>, DomainError> {
            Ok(self.measurements.clone())
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
            from: DateTime<Utc>,
            to: DateTime<Utc>,
            channel_id: Option<measurement_vo::ChannelId>,
        ) -> Result<i64, DomainError> {
            Ok(self
                .measurements
                .iter()
                .filter(|m| m.timestamp.0 >= from && m.timestamp.0 <= to)
                .filter(|m| channel_id.is_none_or(|id| m.channel_id == id))
                .map(|m| m.value.0)
                .sum())
        }

        fn sum_buckets(
            &self,
            _from: DateTime<Utc>,
            _to: DateTime<Utc>,
            _bucket_seconds: i64,
            _origin: DateTime<Utc>,
            _timezone: &str,
            _channel_ids: &[measurement_vo::ChannelId],
        ) -> Result<Vec<crate::core::domain::measurements::repository_port::TimeBucket>, DomainError>
        {
            Ok(Vec::new())
        }

        fn sum_buckets_by_channel(
            &self,
            _from: DateTime<Utc>,
            _to: DateTime<Utc>,
            _bucket_seconds: i64,
            _origin: DateTime<Utc>,
            _timezone: &str,
            _channel_ids: &[measurement_vo::ChannelId],
        ) -> Result<
            Vec<crate::core::domain::measurements::repository_port::ChannelBucket>,
            DomainError,
        > {
            Ok(Vec::new())
        }

        fn sum_weekdays(
            &self,
            _from: DateTime<Utc>,
            _to: DateTime<Utc>,
            _timezone: &str,
            _channel_ids: &[measurement_vo::ChannelId],
        ) -> Result<
            Vec<crate::core::domain::measurements::repository_port::WeekdayTotal>,
            DomainError,
        > {
            Ok(Vec::new())
        }

        fn sum_by_channel(
            &self,
            _from: DateTime<Utc>,
            _to: DateTime<Utc>,
            _channel_ids: &[measurement_vo::ChannelId],
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
        ) -> Result<Vec<crate::core::domain::measurements::repository_port::MonthTotal>, DomainError>
        {
            Ok(Vec::new())
        }
    }

    struct MemoryJobRepository {
        jobs: Vec<Job>,
    }

    impl JobRepository for MemoryJobRepository {
        fn insert(&self, _job: Job) -> Result<(), DomainError> {
            Ok(())
        }
        fn set_running(&self, _id: Uuid, _started_at: DateTime<Utc>) -> Result<(), DomainError> {
            Ok(())
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
        fn update_metadata(
            &self,
            _id: Uuid,
            _key: &str,
            _value: serde_json::Value,
        ) -> Result<(), DomainError> {
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
            Ok(self.jobs.clone())
        }
        fn find_running_by_type(&self, _job_type: &str) -> Result<Option<Job>, DomainError> {
            Ok(None)
        }
        fn find_last_finished_by_type(&self, job_type: &str) -> Result<Option<Job>, DomainError> {
            Ok(self
                .jobs
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

    fn service(measurements: Vec<Measurement>) -> GlobalSummaryService {
        GlobalSummaryService::new(
            Arc::new(MemoryCountingStationRepository {
                stations: vec![station(0x1, "A"), station(0x2, "B")],
            }),
            Arc::new(MemoryChannelRepository {
                channels: vec![channel(0x11, 0x1), channel(0x12, 0x1), channel(0x13, 0x2)],
            }),
            Arc::new(MemoryMeasurementRepository { measurements }),
            Arc::new(MemoryJobRepository {
                jobs: vec![finished_job(0x31, utc(2024, 1, 2, 10, 0, 0))],
            }),
        )
    }

    #[test]
    fn summarize_returns_global_counts_and_last_update() {
        let summary = service(vec![
            // Inside yesterday (Berlin 2024-01-01): 12:00 and 08:00 local.
            measurement(0x21, 0x11, 10, utc(2024, 1, 1, 11, 0, 0)),
            measurement(0x22, 0x11, 5, utc(2024, 1, 1, 7, 0, 0)),
            // Today (Berlin 2024-01-02 10:00 local): excluded.
            measurement(0x23, 0x12, 3, utc(2024, 1, 2, 9, 0, 0)),
        ])
        .summarize(now())
        .unwrap();

        assert_eq!(summary.station_count, 2);
        assert_eq!(summary.channel_count, 3);
        assert_eq!(summary.bikes_last_day_total, 15);
        assert_eq!(summary.last_update, Some(utc(2024, 1, 2, 10, 0, 0)));
    }

    #[test]
    fn summarize_has_no_last_update_without_finished_jobs() {
        let summary = GlobalSummaryService::new(
            Arc::new(MemoryCountingStationRepository { stations: vec![] }),
            Arc::new(MemoryChannelRepository { channels: vec![] }),
            Arc::new(MemoryMeasurementRepository {
                measurements: vec![],
            }),
            Arc::new(MemoryJobRepository { jobs: vec![] }),
        )
        .summarize(now())
        .unwrap();

        assert_eq!(summary.station_count, 0);
        assert_eq!(summary.channel_count, 0);
        assert_eq!(summary.bikes_last_day_total, 0);
        assert_eq!(summary.last_update, None);
    }

    #[test]
    fn summarize_sums_all_channels_in_the_window() {
        // One station A with one channel; a measurement inside yesterday.
        let summary = GlobalSummaryService::new(
            Arc::new(MemoryCountingStationRepository {
                stations: vec![station(0x1, "A")],
            }),
            Arc::new(MemoryChannelRepository {
                channels: vec![channel(0x11, 0x1)],
            }),
            Arc::new(MemoryMeasurementRepository {
                measurements: vec![measurement(0x41, 0x11, 7, utc(2024, 1, 1, 11, 0, 0))],
            }),
            Arc::new(MemoryJobRepository { jobs: vec![] }),
        )
        .summarize(now())
        .unwrap();

        assert_eq!(summary.bikes_last_day_total, 7);
    }
}
