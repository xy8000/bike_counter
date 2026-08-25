//! Application service computing the per-station overview page: the station's
//! channel count, one trend window per metric (each over a complete calendar
//! period in the station's own timezone, plus the immediately preceding period
//! of equal length) and the last successful data-source update timestamp.
//!
//! The BFF handler resolves the image URL from `station.image_asset_id`; this
//! service only deals with measurements and station metadata.

use std::sync::Arc;

use chrono::{DateTime, Duration, Utc};
use chrono_tz::Tz;

use crate::core::application::data_source_update_service::DATA_SOURCE_UPDATE_JOB_TYPE;
use crate::core::domain::channels::channel::value_objects::CountingStationId;
use crate::core::domain::channels::repository_port::ChannelRepository;
use crate::core::domain::counting_stations::counting_station::value_objects::Id;
use crate::core::domain::counting_stations::counting_station::{
    calendar_month_window, previous_calendar_month, previous_local_days,
};
use crate::core::domain::counting_stations::repository_port::CountingStationRepository;
use crate::core::domain::error::DomainError;
use crate::core::domain::jobs::repository_port::JobRepository;
use crate::core::domain::measurements::measurement::value_objects::ChannelId;
use crate::core::domain::measurements::repository_port::MeasurementRepository;
use crate::core::domain::station_overview::service_port::StationOverviewServicePort;
use crate::core::domain::station_overview::{MetricKey, MetricWindow, StationOverview};

pub struct StationOverviewService {
    counting_station_repository: Arc<dyn CountingStationRepository + Send + Sync>,
    channel_repository: Arc<dyn ChannelRepository + Send + Sync>,
    measurement_repository: Arc<dyn MeasurementRepository + Send + Sync>,
    job_repository: Arc<dyn JobRepository + Send + Sync>,
}

impl StationOverviewService {
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

    /// Sums a metric window across every channel of the station.
    fn sum_window(
        &self,
        from: DateTime<Utc>,
        to: DateTime<Utc>,
        channel_ids: &[ChannelId],
    ) -> Result<i64, DomainError> {
        let mut total = 0i64;
        for channel_id in channel_ids {
            total += self
                .measurement_repository
                .sum(from, to, Some(*channel_id))?;
        }
        Ok(total)
    }
}

impl StationOverviewServicePort for StationOverviewService {
    fn overview(&self, station_id: Id, now: DateTime<Utc>) -> Result<StationOverview, DomainError> {
        let station = self.counting_station_repository.find_by_id(station_id)?;
        let tz: Tz = station.timezone.parse()?;

        let channels = self
            .channel_repository
            .find_by_counting_station_id(CountingStationId(station.id.0))?;
        let channel_ids: Vec<ChannelId> = channels
            .iter()
            .map(|channel| ChannelId(channel.id.0))
            .collect();
        let channel_count = channels.len();

        // Previous full local day, and the day before it (comparison period).
        let (day_from, day_to) = previous_local_days(tz, now, 1)?;
        let (before_day_from, _) = previous_local_days(tz, now, 2)?;
        let day = MetricWindow {
            key: MetricKey::LastDay,
            current: self.sum_window(day_from, day_to, &channel_ids)?,
            previous: self.sum_window(
                before_day_from,
                day_from - Duration::microseconds(1),
                &channel_ids,
            )?,
        };

        // Previous 7 full local days, and the 7 days before that.
        let (week_from, week_to) = previous_local_days(tz, now, 7)?;
        let (before_week_from, _) = previous_local_days(tz, now, 14)?;
        let week = MetricWindow {
            key: MetricKey::Last7Days,
            current: self.sum_window(week_from, week_to, &channel_ids)?,
            previous: self.sum_window(
                before_week_from,
                week_from - Duration::microseconds(1),
                &channel_ids,
            )?,
        };

        // Previous full calendar month, and the calendar month before that.
        let (month_from, month_to) = previous_calendar_month(tz, now)?;
        let (before_month_from, _) = calendar_month_window(tz, now, 2)?;
        let month = MetricWindow {
            key: MetricKey::LastMonth,
            current: self.sum_window(month_from, month_to, &channel_ids)?,
            previous: self.sum_window(
                before_month_from,
                month_from - Duration::microseconds(1),
                &channel_ids,
            )?,
        };

        let last_update = self
            .job_repository
            .find_last_finished_by_type(DATA_SOURCE_UPDATE_JOB_TYPE)?
            .and_then(|job| job.finished_at);

        Ok(StationOverview {
            station,
            channel_count,
            metrics: vec![day, week, month],
            last_update,
        })
    }
}

#[cfg(test)]
mod tests {
    use std::collections::HashMap;
    use std::sync::Arc;

    use chrono::{DateTime, Utc};
    use uuid::Uuid;

    use super::*;
    use crate::core::domain::channels::channel::Channel;
    use crate::core::domain::channels::channel::value_objects as channel_vo;
    use crate::core::domain::counting_stations::counting_station::CountingStation;
    use crate::core::domain::counting_stations::counting_station::value_objects as station_vo;
    use crate::core::domain::jobs::job::{Job, JobStatus};
    use crate::core::domain::measurements::measurement::Measurement;
    use crate::core::domain::measurements::measurement::value_objects as measurement_vo;

    const STATION_ID: u128 = 0x1;
    const CHANNEL_ID: u128 = 0x10;

    fn utc(y: i32, mo: u32, d: u32, h: u32, mi: u32, s: u32) -> DateTime<Utc> {
        chrono::TimeZone::with_ymd_and_hms(&Utc, y, mo, d, h, mi, s)
            .single()
            .unwrap()
    }

    fn station() -> CountingStation {
        CountingStation {
            id: station_vo::Id(Uuid::from_u128(STATION_ID)),
            name: station_vo::Name("Promenade".to_string()),
            description: station_vo::Description("desc".to_string()),
            external_datasource_id: None,
            data_source_id: None,
            coordinates: None,
            timezone: station_vo::Timezone("Europe/Berlin".to_string()),
            image_asset_id: None,
            image_sha256: None,
        }
    }

    fn channel() -> Channel {
        Channel {
            id: channel_vo::Id(Uuid::from_u128(CHANNEL_ID)),
            counting_station_id: channel_vo::CountingStationId(Uuid::from_u128(STATION_ID)),
            name: channel_vo::Name("Northbound".to_string()),
            description: channel_vo::Description("desc".to_string()),
            external_datasource_id: None,
        }
    }

    fn measurement(value: i64, when: DateTime<Utc>) -> Measurement {
        Measurement {
            id: measurement_vo::Id(Uuid::new_v4()),
            value: measurement_vo::Value(value),
            channel_id: measurement_vo::ChannelId(Uuid::from_u128(CHANNEL_ID)),
            timestamp: measurement_vo::Timestamp(when),
        }
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
        fn find_by_id(&self, id: station_vo::Id) -> Result<CountingStation, DomainError> {
            self.stations
                .iter()
                .find(|station| station.id == id)
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
            station_id: channel_vo::CountingStationId,
        ) -> Result<Vec<Channel>, DomainError> {
            Ok(self
                .channels
                .iter()
                .filter(|channel| channel.counting_station_id == station_id)
                .cloned()
                .collect())
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

    fn service(measurements: Vec<Measurement>, jobs: Vec<Job>) -> StationOverviewService {
        StationOverviewService::new(
            Arc::new(MemoryCountingStationRepository {
                stations: vec![station()],
            }),
            Arc::new(MemoryChannelRepository {
                channels: vec![channel()],
            }),
            Arc::new(MemoryMeasurementRepository { measurements }),
            Arc::new(MemoryJobRepository { jobs }),
        )
    }

    fn finished_job(finished_at: DateTime<Utc>) -> Job {
        let mut job = Job::new(
            Uuid::new_v4(),
            "data source update".to_string(),
            DATA_SOURCE_UPDATE_JOB_TYPE.to_string(),
            Utc::now() + chrono::Duration::minutes(10),
        );
        job.status = JobStatus::Finished;
        job.started_at = Some(finished_at - chrono::Duration::seconds(10));
        job.finished_at = Some(finished_at);
        job
    }

    #[test]
    fn overview_computes_all_three_metrics_and_channel_count() {
        // now = 2024-01-11 12:00 UTC (Berlin, CET): "yesterday" = 2024-01-10,
        // previous 7 days = Jan 4-10 (previous 7: Dec 28 - Jan 3), previous
        // month = December 2023 (before: November).
        let now = utc(2024, 1, 11, 12, 0, 0);

        let in_day = utc(2024, 1, 10, 12, 0, 0); // last-day window
        let in_day_prev = utc(2024, 1, 9, 12, 0, 0); // day before
        let in_week_only = utc(2024, 1, 4, 12, 0, 0); // last 7 days, not yesterday
        let in_week_prev = utc(2024, 1, 2, 12, 0, 0); // 7 days before
        let in_month = utc(2023, 12, 15, 12, 0, 0); // December
        let in_month_prev = utc(2023, 11, 15, 12, 0, 0); // November

        let measurements = vec![
            measurement(100, in_day),
            measurement(50, in_day_prev),
            measurement(30, in_week_only),
            measurement(20, in_week_prev),
            measurement(5, in_month),
            measurement(2, in_month_prev),
        ];
        let last_update = utc(2024, 1, 11, 6, 0, 0);

        let overview = service(measurements, vec![finished_job(last_update)])
            .overview(station_vo::Id(Uuid::from_u128(STATION_ID)), now)
            .unwrap();

        assert_eq!(overview.channel_count, 1);
        assert_eq!(overview.station.id.0, Uuid::from_u128(STATION_ID));
        assert_eq!(overview.last_update, Some(last_update));

        let by_key: HashMap<_, _> = overview
            .metrics
            .iter()
            .map(|window| (window.key, window))
            .collect();

        // LastDay: yesterday vs the day before.
        assert_eq!(by_key[&MetricKey::LastDay].current, 100);
        assert_eq!(by_key[&MetricKey::LastDay].previous, 50);
        // Last7Days: Jan 4-10 (100 + 50 + 30) vs Dec 28 - Jan 3 (20).
        assert_eq!(by_key[&MetricKey::Last7Days].current, 180);
        assert_eq!(by_key[&MetricKey::Last7Days].previous, 20);
        // LastMonth: December vs November.
        assert_eq!(by_key[&MetricKey::LastMonth].current, 5);
        assert_eq!(by_key[&MetricKey::LastMonth].previous, 2);
    }

    #[test]
    fn overview_has_no_last_update_without_finished_jobs() {
        let now = utc(2024, 1, 11, 12, 0, 0);
        let overview = service(Vec::new(), Vec::new())
            .overview(station_vo::Id(Uuid::from_u128(STATION_ID)), now)
            .unwrap();
        assert_eq!(overview.last_update, None);
        assert_eq!(overview.channel_count, 1);
        assert_eq!(overview.metrics.len(), 3);
    }

    #[test]
    fn overview_unknown_station_is_an_error() {
        let now = utc(2024, 1, 11, 12, 0, 0);
        let service = StationOverviewService::new(
            Arc::new(MemoryCountingStationRepository { stations: vec![] }),
            Arc::new(MemoryChannelRepository { channels: vec![] }),
            Arc::new(MemoryMeasurementRepository {
                measurements: vec![],
            }),
            Arc::new(MemoryJobRepository { jobs: vec![] }),
        );
        let result = service.overview(station_vo::Id(Uuid::from_u128(0x999)), now);
        assert!(matches!(result, Err(DomainError::NotFound(_))));
    }

    #[test]
    fn overview_metrics_follow_the_station_timezone() {
        // A New York station: the windows must be in America/New_York, not the
        // test default. now = 2024-01-02 12:00 UTC = 07:00 EST -> yesterday is
        // 2024-01-01 local.
        let mut ny_station = station();
        ny_station.timezone = station_vo::Timezone("America/New_York".to_string());
        let now = utc(2024, 1, 2, 12, 0, 0);
        // 2024-01-01T18:00Z = 2024-01-01 13:00 EST (yesterday, in window).
        let in_window = measurement(77, utc(2024, 1, 1, 18, 0, 0));
        // 2024-01-01T02:00Z = 2023-12-31 21:00 EST (day before yesterday).
        let before_window = measurement(33, utc(2024, 1, 1, 2, 0, 0));

        let service = StationOverviewService::new(
            Arc::new(MemoryCountingStationRepository {
                stations: vec![ny_station],
            }),
            Arc::new(MemoryChannelRepository {
                channels: vec![channel()],
            }),
            Arc::new(MemoryMeasurementRepository {
                measurements: vec![in_window, before_window],
            }),
            Arc::new(MemoryJobRepository { jobs: vec![] }),
        );
        let overview = service
            .overview(station_vo::Id(Uuid::from_u128(STATION_ID)), now)
            .unwrap();
        let day = overview
            .metrics
            .iter()
            .find(|window| window.key == MetricKey::LastDay)
            .unwrap();
        assert_eq!(day.current, 77);
        assert_eq!(day.previous, 33);
    }

    #[test]
    fn metric_keys_serialize_to_stable_strings() {
        assert_eq!(MetricKey::LastDay.as_str(), "last_day");
        assert_eq!(MetricKey::Last7Days.as_str(), "last_7_days");
        assert_eq!(MetricKey::LastMonth.as_str(), "last_month");
    }
}
