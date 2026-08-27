use std::collections::{BTreeMap, HashMap};
use std::sync::Arc;

use chrono::{DateTime, Datelike, TimeZone, Timelike, Utc};
use uuid::Uuid;

use super::service::StationAnalyticsService;
use crate::core::application::data_source_update_service::DATA_SOURCE_UPDATE_JOB_TYPE;
use crate::core::domain::channels::channel::Channel;
use crate::core::domain::channels::channel::value_objects as channel_vo;
use crate::core::domain::channels::repository_port::ChannelRepository;
use crate::core::domain::counting_stations::counting_station::CountingStation;
use crate::core::domain::counting_stations::counting_station::value_objects as station_vo;
use crate::core::domain::counting_stations::repository_port::CountingStationRepository;
use crate::core::domain::error::DomainError;
use crate::core::domain::jobs::job::{Job, JobStatus};
use crate::core::domain::jobs::repository_port::JobRepository;
use crate::core::domain::measurements::measurement::Measurement;
use crate::core::domain::measurements::measurement::value_objects as measurement_vo;
use crate::core::domain::measurements::repository_port::{
    ChannelBucket, ChannelHourTotal, ChannelTotal, HourTotal, MeasurementRepository, MonthTotal,
    ResolutionCoverage, TimeBucket, WeekdayTotal,
};
use crate::core::domain::station_analytics::service_port::StationAnalyticsServicePort;
use crate::core::domain::station_analytics::{
    GeoBounds, GraphTimeframe, MetricKey, MetricWindow, StationsSummaryOverview,
};

const STATION_1: u128 = 0x1;
const STATION_B: u128 = 0x2;
const STATION_C: u128 = 0x3;
const CHANNEL_A1: u128 = 0x11;
const CHANNEL_A2: u128 = 0x12;
const CHANNEL_B1: u128 = 0x13;
const CHANNEL_C1: u128 = 0x14;

fn utc(y: i32, mo: u32, d: u32, h: u32, mi: u32, s: u32) -> DateTime<Utc> {
    Utc.with_ymd_and_hms(y, mo, d, h, mi, s).single().unwrap()
}

fn station(id: u128, name: &str, coordinates: Option<(f64, f64)>) -> CountingStation {
    CountingStation {
        id: station_vo::Id(Uuid::from_u128(id)),
        name: station_vo::Name(name.to_string()),
        description: station_vo::Description(format!("{name} description")),
        external_datasource_id: None,
        data_source_id: None,
        coordinates: coordinates.map(|(latitude, longitude)| station_vo::GeoCoordinates {
            latitude,
            longitude,
        }),
        timezone: station_vo::Timezone("Europe/Berlin".to_string()),
        image_asset_id: None,
        image_sha256: None,
    }
}

fn channel(id: u128, station_id: u128, name: &str) -> Channel {
    Channel {
        id: channel_vo::Id(Uuid::from_u128(id)),
        counting_station_id: channel_vo::CountingStationId(Uuid::from_u128(station_id)),
        name: channel_vo::Name(name.to_string()),
        description: channel_vo::Description(String::new()),
        external_datasource_id: None,
    }
}

fn measurement(channel_id: u128, value: i64, when: DateTime<Utc>) -> Measurement {
    Measurement {
        id: measurement_vo::Id(Uuid::new_v4()),
        value: measurement_vo::Value(value),
        channel_id: measurement_vo::ChannelId(Uuid::from_u128(channel_id)),
        timestamp: measurement_vo::Timestamp(when),
        resolution_seconds: measurement_vo::ResolutionSeconds(3600),
        interval_end: None,
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

/// In-memory measurement repository with UTC (timezone-naive) bucketing that
/// mirrors the Postgres `date_bin` semantics closely enough for the service
/// tests.
struct MemoryMeasurementRepository {
    measurements: Vec<Measurement>,
}

impl MemoryMeasurementRepository {
    fn in_window(
        &self,
        from: DateTime<Utc>,
        to: DateTime<Utc>,
        channel_ids: &[measurement_vo::ChannelId],
    ) -> impl Iterator<Item = &Measurement> {
        self.measurements.iter().filter(move |m| {
            m.timestamp.0 >= from
                && m.timestamp.0 <= to
                && channel_ids.iter().any(|id| id.0 == m.channel_id.0)
        })
    }

    fn bucket_start(
        timestamp: DateTime<Utc>,
        origin: DateTime<Utc>,
        bucket_seconds: i64,
    ) -> DateTime<Utc> {
        let elapsed = timestamp.signed_duration_since(origin).num_seconds();
        let index = elapsed.div_euclid(bucket_seconds);
        origin + chrono::Duration::seconds(index * bucket_seconds)
    }
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
        channel_ids: &[measurement_vo::ChannelId],
        resolution_seconds: Option<i64>,
    ) -> Result<i64, DomainError> {
        Ok(self
            .measurements
            .iter()
            .filter(|m| m.timestamp.0 >= from && m.timestamp.0 <= to)
            .filter(|m| channel_ids.contains(&m.channel_id))
            .filter(|m| resolution_seconds.is_none_or(|r| m.resolution_seconds.0 == r))
            .map(|m| m.value.0)
            .sum())
    }

    fn sum_buckets(
        &self,
        from: DateTime<Utc>,
        to: DateTime<Utc>,
        bucket_seconds: i64,
        origin: DateTime<Utc>,
        _timezone: &str,
        channel_ids: &[measurement_vo::ChannelId],
        resolution_seconds: Option<i64>,
    ) -> Result<Vec<TimeBucket>, DomainError> {
        let mut map: BTreeMap<DateTime<Utc>, i64> = BTreeMap::new();
        for m in self
            .in_window(from, to, channel_ids)
            .filter(|m| resolution_seconds.is_none_or(|r| m.resolution_seconds.0 == r))
        {
            let start = Self::bucket_start(m.timestamp.0, origin, bucket_seconds);
            *map.entry(start).or_insert(0) += m.value.0;
        }
        Ok(map
            .into_iter()
            .map(|(start, total)| TimeBucket { start, total })
            .collect())
    }

    fn sum_buckets_by_channel(
        &self,
        from: DateTime<Utc>,
        to: DateTime<Utc>,
        bucket_seconds: i64,
        origin: DateTime<Utc>,
        _timezone: &str,
        channel_ids: &[measurement_vo::ChannelId],
        resolution_seconds: Option<i64>,
    ) -> Result<Vec<ChannelBucket>, DomainError> {
        let mut map: BTreeMap<(Uuid, DateTime<Utc>), i64> = BTreeMap::new();
        for m in self
            .in_window(from, to, channel_ids)
            .filter(|m| resolution_seconds.is_none_or(|r| m.resolution_seconds.0 == r))
        {
            let start = Self::bucket_start(m.timestamp.0, origin, bucket_seconds);
            *map.entry((m.channel_id.0, start)).or_insert(0) += m.value.0;
        }
        Ok(map
            .into_iter()
            .map(|((channel_id, start), total)| ChannelBucket {
                channel_id,
                start,
                total,
            })
            .collect())
    }

    fn sum_weekdays(
        &self,
        from: DateTime<Utc>,
        to: DateTime<Utc>,
        _timezone: &str,
        channel_ids: &[measurement_vo::ChannelId],
        resolution_seconds: Option<i64>,
    ) -> Result<Vec<WeekdayTotal>, DomainError> {
        let mut map: BTreeMap<u8, i64> = BTreeMap::new();
        for m in self
            .in_window(from, to, channel_ids)
            .filter(|m| resolution_seconds.is_none_or(|r| m.resolution_seconds.0 == r))
        {
            let weekday = (m.timestamp.0.weekday().num_days_from_monday() + 1) as u8;
            *map.entry(weekday).or_insert(0) += m.value.0;
        }
        Ok(map
            .into_iter()
            .map(|(weekday, total)| WeekdayTotal { weekday, total })
            .collect())
    }

    fn sum_hours(
        &self,
        from: DateTime<Utc>,
        to: DateTime<Utc>,
        _timezone: &str,
        channel_ids: &[measurement_vo::ChannelId],
        resolution_seconds: Option<i64>,
    ) -> Result<Vec<HourTotal>, DomainError> {
        let mut map: BTreeMap<u8, i64> = BTreeMap::new();
        for m in self
            .in_window(from, to, channel_ids)
            .filter(|m| resolution_seconds.is_none_or(|r| m.resolution_seconds.0 == r))
        {
            let hour = m.timestamp.0.hour() as u8;
            *map.entry(hour).or_insert(0) += m.value.0;
        }
        Ok(map
            .into_iter()
            .map(|(hour, total)| HourTotal { hour, total })
            .collect())
    }

    fn sum_hours_by_channel(
        &self,
        from: DateTime<Utc>,
        to: DateTime<Utc>,
        _timezone: &str,
        channel_ids: &[measurement_vo::ChannelId],
        resolution_seconds: Option<i64>,
    ) -> Result<Vec<ChannelHourTotal>, DomainError> {
        let mut map: BTreeMap<(Uuid, u8), i64> = BTreeMap::new();
        for m in self
            .in_window(from, to, channel_ids)
            .filter(|m| resolution_seconds.is_none_or(|r| m.resolution_seconds.0 == r))
        {
            let hour = m.timestamp.0.hour() as u8;
            *map.entry((m.channel_id.0, hour)).or_insert(0) += m.value.0;
        }
        Ok(map
            .into_iter()
            .map(|((channel_id, hour), total)| ChannelHourTotal {
                channel_id,
                hour,
                total,
            })
            .collect())
    }
    fn sum_by_channel(
        &self,
        from: DateTime<Utc>,
        to: DateTime<Utc>,
        channel_ids: &[measurement_vo::ChannelId],
        resolution_seconds: Option<i64>,
    ) -> Result<Vec<ChannelTotal>, DomainError> {
        let mut map: BTreeMap<Uuid, i64> = BTreeMap::new();
        for m in self
            .in_window(from, to, channel_ids)
            .filter(|m| resolution_seconds.is_none_or(|r| m.resolution_seconds.0 == r))
        {
            *map.entry(m.channel_id.0).or_insert(0) += m.value.0;
        }
        Ok(map
            .into_iter()
            .map(|(channel_id, total)| ChannelTotal { channel_id, total })
            .collect())
    }

    fn sum_by_month(
        &self,
        timezone: &str,
        channel_ids: &[measurement_vo::ChannelId],
        resolution_seconds: Option<i64>,
    ) -> Result<Vec<MonthTotal>, DomainError> {
        let tz: chrono_tz::Tz = timezone.parse().map_err(|_| {
            DomainError::InvalidQuery(format!("unknown IANA timezone '{timezone}'"))
        })?;
        let mut map: BTreeMap<(i32, u32), i64> = BTreeMap::new();
        for m in self
            .measurements
            .iter()
            .filter(|m| channel_ids.iter().any(|id| id.0 == m.channel_id.0))
            .filter(|m| resolution_seconds.is_none_or(|r| m.resolution_seconds.0 == r))
        {
            let local = m.timestamp.0.with_timezone(&tz);
            *map.entry((local.year(), local.month())).or_insert(0) += m.value.0;
        }
        Ok(map
            .into_iter()
            .map(|((year, month), total)| MonthTotal {
                year,
                month: month as u8,
                total,
            })
            .collect())
    }

    fn resolution_coverage(
        &self,
        from: DateTime<Utc>,
        to: DateTime<Utc>,
        channel_ids: &[measurement_vo::ChannelId],
    ) -> Result<Vec<ResolutionCoverage>, DomainError> {
        let mut map: BTreeMap<i64, (DateTime<Utc>, DateTime<Utc>, i64)> = BTreeMap::new();
        for m in self.in_window(from, to, channel_ids) {
            let r = m.resolution_seconds.0;
            let entry = map.entry(r).or_insert((m.timestamp.0, m.timestamp.0, 0));
            entry.0 = entry.0.min(m.timestamp.0);
            entry.1 = entry.1.max(m.timestamp.0);
            entry.2 += 1;
        }
        Ok(map
            .into_iter()
            .map(
                |(resolution_seconds, (first, last, count))| ResolutionCoverage {
                    resolution_seconds,
                    first,
                    last,
                    count,
                },
            )
            .collect())
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

fn service(
    stations: Vec<CountingStation>,
    channels: Vec<Channel>,
    measurements: Vec<Measurement>,
    jobs: Vec<Job>,
) -> StationAnalyticsService {
    StationAnalyticsService::new(
        Arc::new(MemoryCountingStationRepository { stations }),
        Arc::new(MemoryChannelRepository { channels }),
        Arc::new(MemoryMeasurementRepository { measurements }),
        Arc::new(MemoryJobRepository { jobs }),
    )
}

fn promenade_service(measurements: Vec<Measurement>) -> StationAnalyticsService {
    service(
        vec![station(STATION_1, "Promenade", None)],
        vec![
            channel(CHANNEL_A1, STATION_1, "Northbound"),
            channel(CHANNEL_A2, STATION_1, "Southbound"),
        ],
        measurements,
        vec![],
    )
}

/// Fixed "now": 2024-01-11 12:00 UTC (Berlin).
fn detail_now() -> DateTime<Utc> {
    utc(2024, 1, 11, 12, 0, 0)
}

fn sum_buckets(series: &[TimeBucket]) -> i64 {
    series.iter().map(|bucket| bucket.total).sum()
}

fn bounds() -> GeoBounds {
    GeoBounds {
        min_latitude: 51.9,
        min_longitude: 7.5,
        max_latitude: 52.0,
        max_longitude: 7.8,
    }
}

// -----------------------------------------------------------------------
// station summaries (sidebar / search)
// -----------------------------------------------------------------------

#[test]
fn summaries_in_bounds_filters_stations_and_counts_channels() {
    let service = service(
        vec![
            station(STATION_1, "A", Some((51.96, 7.63))),
            station(STATION_B, "B", None),
        ],
        vec![
            channel(CHANNEL_A1, STATION_1, "a1"),
            channel(CHANNEL_A2, STATION_1, "a2"),
            channel(CHANNEL_B1, STATION_B, "b1"),
        ],
        vec![],
        vec![],
    );

    let summaries = service
        .summaries(Some(bounds()), utc(2024, 1, 2, 12, 0, 0))
        .unwrap();
    assert_eq!(summaries.len(), 1, "only station A lies inside the bounds");
    let summary = &summaries[0];
    assert_eq!(summary.station.id.0, Uuid::from_u128(STATION_1));
    assert_eq!(summary.station.name.0, "A");
    assert_eq!(summary.channel_count, 2);
}

#[test]
fn summaries_bikes_last_day_sums_only_measurements_inside_the_window() {
    let service = service(
        vec![station(STATION_1, "A", Some((51.96, 7.63)))],
        vec![
            channel(CHANNEL_A1, STATION_1, "a1"),
            channel(CHANNEL_A2, STATION_1, "a2"),
        ],
        vec![
            measurement(CHANNEL_A1, 10, utc(2024, 1, 1, 11, 0, 0)),
            measurement(CHANNEL_A1, 5, utc(2024, 1, 1, 7, 0, 0)),
            measurement(CHANNEL_A1, 100, utc(2023, 12, 31, 11, 0, 0)),
            measurement(CHANNEL_A2, 3, utc(2024, 1, 2, 9, 0, 0)),
        ],
        vec![],
    );
    // now = 2024-01-02 12:00 UTC (Berlin): yesterday is 2024-01-01 local.
    let summaries = service
        .summaries(Some(bounds()), utc(2024, 1, 2, 12, 0, 0))
        .unwrap();
    assert_eq!(summaries.len(), 1);
    assert_eq!(summaries[0].bikes_last_day, 15);
}

#[test]
fn summaries_all_includes_stations_without_coordinates() {
    let service = service(
        vec![
            station(STATION_1, "A", Some((51.96, 7.63))),
            station(STATION_B, "B", None),
        ],
        vec![
            channel(CHANNEL_A1, STATION_1, "a1"),
            channel(CHANNEL_B1, STATION_B, "b1"),
        ],
        vec![],
        vec![],
    );
    let summaries = service.summaries(None, utc(2024, 1, 2, 12, 0, 0)).unwrap();
    assert_eq!(summaries.len(), 2, "all stations are returned");
    let station_b = summaries
        .iter()
        .find(|s| s.station.id.0 == Uuid::from_u128(STATION_B))
        .expect("station B present");
    assert_eq!(station_b.station.coordinates, None);
    assert_eq!(station_b.channel_count, 1);
    assert_eq!(station_b.bikes_last_day, 0, "station B has no measurements");
}

#[test]
fn summaries_per_station_timezone_uses_each_station_local_day() {
    let mut station_c = station(STATION_C, "C", Some((51.99, 7.6)));
    station_c.timezone = station_vo::Timezone("America/New_York".to_string());
    let service = service(
        vec![station(STATION_1, "A", Some((51.96, 7.63))), station_c],
        vec![
            channel(CHANNEL_A1, STATION_1, "a1"),
            channel(CHANNEL_C1, STATION_C, "c1"),
        ],
        vec![
            measurement(CHANNEL_A1, 99, utc(2024, 1, 1, 23, 30, 0)),
            measurement(CHANNEL_C1, 99, utc(2024, 1, 1, 23, 30, 0)),
        ],
        vec![],
    );
    // 2024-01-01T23:30Z is 00:30 on Jan 2 in Berlin (today -> excluded) but
    // 18:30 on Jan 1 in New York (yesterday -> included).
    let summaries = service.summaries(None, utc(2024, 1, 2, 12, 0, 0)).unwrap();
    let by_name: HashMap<_, _> = summaries
        .iter()
        .map(|s| (s.station.name.0.clone(), s.bikes_last_day))
        .collect();
    assert_eq!(by_name.get("A"), Some(&0));
    assert_eq!(by_name.get("C"), Some(&99));
}

#[test]
fn sidebar_shell_returns_only_in_bounds_stations_sorted_by_name() {
    let service = service(
        vec![
            station(STATION_1, "B", Some((51.96, 7.63))),
            station(STATION_B, "A", Some((51.98, 7.6))),
            station(STATION_C, "NoCoords", None),
        ],
        vec![],
        vec![],
        vec![],
    );

    let shell = service.sidebar_shell(bounds()).unwrap();
    assert_eq!(
        shell.iter().map(|s| s.name.0.as_str()).collect::<Vec<_>>(),
        vec!["A", "B"],
        "only positioned in-bounds stations, sorted by name"
    );
    assert!(
        shell.iter().all(|s| s.coordinates.is_some()),
        "stations without coordinates are not in the shell"
    );
}

#[test]
fn sidebar_stats_returns_channel_counts_and_bikes_last_day_per_station() {
    let service = service(
        vec![
            station(STATION_1, "A", Some((51.96, 7.63))),
            station(STATION_B, "B", None),
        ],
        vec![
            channel(CHANNEL_A1, STATION_1, "a1"),
            channel(CHANNEL_A2, STATION_1, "a2"),
            channel(CHANNEL_B1, STATION_B, "b1"),
        ],
        vec![
            measurement(CHANNEL_A1, 10, utc(2024, 1, 1, 11, 0, 0)),
            measurement(CHANNEL_A1, 5, utc(2024, 1, 1, 7, 0, 0)),
            measurement(CHANNEL_A2, 3, utc(2024, 1, 2, 9, 0, 0)),
        ],
        vec![],
    );

    let stats = service
        .sidebar_stats(bounds(), utc(2024, 1, 2, 12, 0, 0))
        .unwrap();
    assert_eq!(stats.len(), 1, "only station A lies inside the bounds");
    assert_eq!(stats[0].station_id, Uuid::from_u128(STATION_1));
    assert_eq!(stats[0].channel_count, 2);
    assert_eq!(stats[0].bikes_last_day, 15);
}

// -----------------------------------------------------------------------
// global summary
// -----------------------------------------------------------------------

#[test]
fn global_summary_returns_global_counts_and_last_update() {
    let service = service(
        vec![station(STATION_1, "A", None), station(STATION_B, "B", None)],
        vec![
            channel(CHANNEL_A1, STATION_1, "a1"),
            channel(CHANNEL_A2, STATION_1, "a2"),
            channel(CHANNEL_B1, STATION_B, "b1"),
        ],
        vec![
            measurement(CHANNEL_A1, 10, utc(2024, 1, 1, 11, 0, 0)),
            measurement(CHANNEL_A1, 5, utc(2024, 1, 1, 7, 0, 0)),
            measurement(CHANNEL_A2, 3, utc(2024, 1, 2, 9, 0, 0)),
        ],
        vec![finished_job(0x31, utc(2024, 1, 2, 10, 0, 0))],
    );
    let summary = service.global_summary(utc(2024, 1, 2, 12, 0, 0)).unwrap();
    assert_eq!(summary.station_count, 2);
    assert_eq!(summary.channel_count, 3);
    assert_eq!(summary.bikes_last_day_total, 15);
    assert_eq!(summary.last_update, Some(utc(2024, 1, 2, 10, 0, 0)));
}

#[test]
fn global_summary_has_no_last_update_without_finished_jobs() {
    let summary = service(vec![], vec![], vec![], vec![])
        .global_summary(utc(2024, 1, 2, 12, 0, 0))
        .unwrap();
    assert_eq!(summary.station_count, 0);
    assert_eq!(summary.channel_count, 0);
    assert_eq!(summary.bikes_last_day_total, 0);
    assert_eq!(summary.last_update, None);
}

// -----------------------------------------------------------------------
// station overview page
// -----------------------------------------------------------------------

#[test]
fn overview_shell_returns_channel_count_and_last_update() {
    let last_update = utc(2024, 1, 11, 6, 0, 0);
    let service = service(
        vec![station(STATION_1, "Promenade", None)],
        vec![
            channel(CHANNEL_A1, STATION_1, "Northbound"),
            channel(CHANNEL_A2, STATION_1, "Southbound"),
        ],
        vec![],
        vec![finished_job(0x51, last_update)],
    );

    let shell = service
        .overview_shell(station_vo::Id(Uuid::from_u128(STATION_1)))
        .unwrap();
    assert_eq!(shell.station.id.0, Uuid::from_u128(STATION_1));
    assert_eq!(shell.channel_count, 2);
    assert_eq!(shell.last_update, Some(last_update));
}

#[test]
fn overview_shell_has_no_last_update_without_finished_jobs() {
    let shell = promenade_service(Vec::new())
        .overview_shell(station_vo::Id(Uuid::from_u128(STATION_1)))
        .unwrap();
    assert_eq!(shell.last_update, None);
    assert_eq!(shell.channel_count, 2);
}

#[test]
fn overview_shell_unknown_station_is_an_error() {
    let result =
        promenade_service(Vec::new()).overview_shell(station_vo::Id(Uuid::from_u128(0x999)));
    assert!(matches!(result, Err(DomainError::NotFound(_))));
}

#[test]
fn overview_stats_metrics_follow_the_station_timezone() {
    let mut ny_station = station(STATION_1, "NY", None);
    ny_station.timezone = station_vo::Timezone("America/New_York".to_string());
    let service = service(
        vec![ny_station],
        vec![channel(CHANNEL_A1, STATION_1, "a1")],
        vec![
            measurement(CHANNEL_A1, 77, utc(2024, 1, 1, 18, 0, 0)),
            measurement(CHANNEL_A1, 33, utc(2024, 1, 1, 2, 0, 0)),
        ],
        vec![],
    );
    // now = 2024-01-02 12:00 UTC = 07:00 EST -> yesterday is 2024-01-01.
    let stats = service
        .detail_overview_stats(
            station_vo::Id(Uuid::from_u128(STATION_1)),
            utc(2024, 1, 2, 12, 0, 0),
        )
        .unwrap();
    let day = stats
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
    assert_eq!(MetricKey::LastYear.as_str(), "last_year");
}

// -----------------------------------------------------------------------
// station detail page
// -----------------------------------------------------------------------

#[test]
fn detail_computes_all_windows() {
    let now = detail_now();
    let measurements = vec![
        measurement(CHANNEL_A1, 100, utc(2024, 1, 10, 12, 0, 0)), // last day
        measurement(CHANNEL_A1, 30, utc(2024, 1, 4, 12, 0, 0)),   // last week
        measurement(CHANNEL_A2, 50, utc(2024, 1, 8, 12, 0, 0)),   // current week (Mon)
        measurement(CHANNEL_A1, 20, utc(2023, 12, 20, 12, 0, 0)), // last 30 days + last year
        measurement(CHANNEL_A1, 10, utc(2024, 1, 5, 12, 0, 0)),   // current year + last week
        measurement(CHANNEL_A1, 5, utc(2023, 6, 15, 12, 0, 0)),   // last year only
    ];
    let service = promenade_service(measurements);
    let id = station_vo::Id(Uuid::from_u128(STATION_1));

    let page = service.detail_page(id, now).unwrap();
    assert_eq!(page.channels.len(), 2);

    let day = service
        .detail_graphs_timeframe(id, GraphTimeframe::Day, now)
        .unwrap();
    assert_eq!(
        sum_buckets(&day.current),
        100,
        "only the Jan 10 measurement"
    );
    assert!(day.previous.is_empty(), "no data for the day before");

    let week = service
        .detail_graphs_timeframe(id, GraphTimeframe::Week, now)
        .unwrap();
    assert_eq!(sum_buckets(&week.current), 150, "Jan 8 (Mon) + Jan 10");
    assert_eq!(sum_buckets(&week.previous), 40, "Jan 4 + Jan 5");

    let last_30_days = service
        .detail_graphs_timeframe(id, GraphTimeframe::Last30Days, now)
        .unwrap();
    assert_eq!(
        sum_buckets(&last_30_days.current),
        210,
        "all but the June 2023 one"
    );
    assert!(
        last_30_days.previous.is_empty(),
        "no data for the 30 days before"
    );

    let year = service
        .detail_graphs_timeframe(id, GraphTimeframe::Year, now)
        .unwrap();
    assert_eq!(sum_buckets(&year.current), 190, "all 2024 measurements");
    assert_eq!(sum_buckets(&year.previous), 25, "Dec 2023 + Jun 2023");
}

#[test]
fn detail_computes_previous_periods_for_day_and_last_30_days() {
    let now = detail_now();
    let measurements = vec![
        // Previous day: 2024-01-09 local = [2024-01-08T23:00Z, 2024-01-09T23:00Z).
        measurement(CHANNEL_A1, 3, utc(2024, 1, 9, 12, 0, 0)),
        // Previous 30 days: 2023-11-12 .. 2023-12-11 local.
        measurement(CHANNEL_A1, 4, utc(2023, 12, 1, 12, 0, 0)),
    ];
    let service = promenade_service(measurements);
    let id = station_vo::Id(Uuid::from_u128(STATION_1));
    let day = service
        .detail_graphs_timeframe(id, GraphTimeframe::Day, now)
        .unwrap();
    let last_30_days = service
        .detail_graphs_timeframe(id, GraphTimeframe::Last30Days, now)
        .unwrap();

    assert!(day.current.is_empty());
    assert_eq!(sum_buckets(&day.previous), 3);
    assert_eq!(sum_buckets(&last_30_days.current), 3);
    assert_eq!(sum_buckets(&last_30_days.previous), 4);

    let day_a = day
        .per_channel
        .iter()
        .find(|s| s.channel_id == Uuid::from_u128(CHANNEL_A1))
        .unwrap();
    assert!(day_a.current.is_empty());
    assert_eq!(sum_buckets(&day_a.previous), 3);
    let thirty_a = last_30_days
        .per_channel
        .iter()
        .find(|s| s.channel_id == Uuid::from_u128(CHANNEL_A1))
        .unwrap();
    assert_eq!(sum_buckets(&thirty_a.current), 3);
    assert_eq!(sum_buckets(&thirty_a.previous), 4);
}

#[test]
fn detail_computes_monthly_totals() {
    let now = detail_now();
    let measurements = vec![
        measurement(CHANNEL_A1, 100, utc(2024, 1, 10, 12, 0, 0)),
        measurement(CHANNEL_A1, 20, utc(2023, 12, 20, 12, 0, 0)),
        measurement(CHANNEL_A2, 5, utc(2023, 12, 21, 12, 0, 0)),
        measurement(CHANNEL_A1, 7, utc(2023, 6, 15, 12, 0, 0)),
    ];
    let monthly = promenade_service(measurements)
        .detail_monthly(station_vo::Id(Uuid::from_u128(STATION_1)), now)
        .unwrap();
    assert_eq!(
        monthly,
        vec![
            MonthTotal {
                year: 2023,
                month: 6,
                total: 7
            },
            MonthTotal {
                year: 2023,
                month: 12,
                total: 25
            },
            MonthTotal {
                year: 2024,
                month: 1,
                total: 100
            },
        ],
        "local calendar month grouping, ascending by year then month"
    );
}

#[test]
fn detail_resolutions_bucket_by_hour_and_day() {
    let now = detail_now();
    let measurements = vec![
        measurement(CHANNEL_A1, 1, utc(2024, 1, 8, 5, 0, 0)),
        measurement(CHANNEL_A1, 2, utc(2024, 1, 8, 6, 0, 0)),
        measurement(CHANNEL_A1, 4, utc(2024, 1, 8, 7, 0, 0)),
        measurement(CHANNEL_A1, 10, utc(2023, 12, 19, 23, 0, 0)),
        measurement(CHANNEL_A1, 20, utc(2023, 12, 20, 23, 0, 0)),
        measurement(CHANNEL_A1, 100, utc(2022, 12, 31, 23, 0, 0)),
        measurement(CHANNEL_A1, 200, utc(2023, 1, 1, 23, 0, 0)),
    ];
    let service = promenade_service(measurements);
    let id = station_vo::Id(Uuid::from_u128(STATION_1));
    let week = service
        .detail_graphs_timeframe(id, GraphTimeframe::Week, now)
        .unwrap();
    let last_30_days = service
        .detail_graphs_timeframe(id, GraphTimeframe::Last30Days, now)
        .unwrap();
    let year = service
        .detail_graphs_timeframe(id, GraphTimeframe::Year, now)
        .unwrap();

    let week_starts: Vec<i64> = week.current.iter().map(|b| b.start.timestamp()).collect();
    assert_eq!(
        week_starts,
        vec![
            utc(2024, 1, 8, 5, 0, 0).timestamp(),
            utc(2024, 1, 8, 6, 0, 0).timestamp(),
            utc(2024, 1, 8, 7, 0, 0).timestamp(),
        ],
        "current week buckets are one hour apart"
    );

    let thirty_day_starts: Vec<i64> = last_30_days
        .current
        .iter()
        .map(|b| b.start.timestamp())
        .collect();
    assert_eq!(
        thirty_day_starts,
        vec![
            utc(2023, 12, 19, 23, 0, 0).timestamp(),
            utc(2023, 12, 20, 23, 0, 0).timestamp(),
            utc(2024, 1, 7, 23, 0, 0).timestamp(),
        ],
        "last 30 days buckets are one day apart"
    );
    let thirty_day_diffs: Vec<i64> = thirty_day_starts
        .windows(2)
        .map(|pair| pair[1] - pair[0])
        .collect();
    assert!(
        thirty_day_diffs.iter().all(|diff| diff % 86_400 == 0),
        "last 30 days buckets are aligned to whole local days (no zero-filling)"
    );

    let last_year_starts: Vec<i64> = year.previous.iter().map(|b| b.start.timestamp()).collect();
    assert_eq!(
        &last_year_starts[..2],
        &[
            utc(2022, 12, 31, 23, 0, 0).timestamp(),
            utc(2023, 1, 1, 23, 0, 0).timestamp(),
        ],
        "last year buckets align to the previous year's local midnights"
    );
    let last_year_diffs: Vec<i64> = last_year_starts
        .windows(2)
        .map(|pair| pair[1] - pair[0])
        .collect();
    assert!(
        last_year_diffs.iter().all(|diff| diff % 86_400 == 0),
        "last year buckets are aligned to whole local days"
    );
}

#[test]
fn detail_current_week_has_no_future_buckets() {
    let now = detail_now();
    let measurements = vec![measurement(CHANNEL_A1, 7, utc(2024, 1, 8, 6, 0, 0))];
    let week = promenade_service(measurements)
        .detail_graphs_timeframe(
            station_vo::Id(Uuid::from_u128(STATION_1)),
            GraphTimeframe::Week,
            now,
        )
        .unwrap();
    assert_eq!(week.current.len(), 1);
    assert_eq!(sum_buckets(&week.current), 7);
}

#[test]
fn detail_per_channel_series_and_pie() {
    let now = detail_now();
    let measurements = vec![
        measurement(CHANNEL_A1, 100, utc(2024, 1, 10, 12, 0, 0)), // last day
        measurement(CHANNEL_A2, 50, utc(2024, 1, 8, 12, 0, 0)),   // current week
        measurement(CHANNEL_A1, 20, utc(2023, 12, 20, 12, 0, 0)), // last 30 days
    ];
    let last_30_days = promenade_service(measurements)
        .detail_graphs_timeframe(
            station_vo::Id(Uuid::from_u128(STATION_1)),
            GraphTimeframe::Last30Days,
            now,
        )
        .unwrap();

    assert_eq!(last_30_days.per_channel.len(), 2, "both channels have data");
    let a = last_30_days
        .per_channel
        .iter()
        .find(|s| s.channel_id == Uuid::from_u128(CHANNEL_A1))
        .unwrap();
    let b = last_30_days
        .per_channel
        .iter()
        .find(|s| s.channel_id == Uuid::from_u128(CHANNEL_A2))
        .unwrap();
    assert_eq!(sum_buckets(&a.current), 120, "100 + 20 over 30 days");
    assert_eq!(sum_buckets(&b.current), 50);

    let pie = &last_30_days.channel_pie;
    assert_eq!(pie.len(), 2);
    let by_id: HashMap<_, _> = pie.iter().map(|c| (c.channel_id, c.total)).collect();
    assert_eq!(by_id[&Uuid::from_u128(CHANNEL_A1)], 120);
    assert_eq!(by_id[&Uuid::from_u128(CHANNEL_A2)], 50);
}

#[test]
fn detail_per_channel_weekday_radar_follows_the_station_timezone() {
    let now = detail_now();
    // 2024-01-10 and 2023-12-20 are both Wednesdays in Europe/Berlin.
    let measurements = vec![
        measurement(CHANNEL_A1, 100, utc(2024, 1, 10, 12, 0, 0)),
        measurement(CHANNEL_A1, 20, utc(2023, 12, 20, 12, 0, 0)),
        measurement(CHANNEL_A2, 50, utc(2024, 1, 8, 12, 0, 0)), // Monday
    ];
    let last_30_days = promenade_service(measurements)
        .detail_graphs_timeframe(
            station_vo::Id(Uuid::from_u128(STATION_1)),
            GraphTimeframe::Last30Days,
            now,
        )
        .unwrap();

    let a = last_30_days
        .per_channel
        .iter()
        .find(|s| s.channel_id == Uuid::from_u128(CHANNEL_A1))
        .unwrap();
    let b = last_30_days
        .per_channel
        .iter()
        .find(|s| s.channel_id == Uuid::from_u128(CHANNEL_A2))
        .unwrap();

    let by_weekday_a: HashMap<_, _> = a
        .weekday_radar
        .iter()
        .map(|w| (w.weekday, w.total))
        .collect();
    assert_eq!(by_weekday_a.len(), 1, "both A measurements are Wednesdays");
    assert_eq!(by_weekday_a.get(&3), Some(&120));
    assert_eq!(b.weekday_radar.len(), 1);
    assert_eq!(b.weekday_radar[0].weekday, 1, "Monday");
    assert_eq!(b.weekday_radar[0].total, 50);
}

#[test]
fn detail_weekday_radar_aggregates_over_last_30_days() {
    let now = detail_now();
    let measurements = vec![
        measurement(CHANNEL_A1, 100, utc(2024, 1, 10, 12, 0, 0)), // Wed (3)
        measurement(CHANNEL_A1, 20, utc(2023, 12, 20, 12, 0, 0)), // Wed (3)
        measurement(CHANNEL_A1, 30, utc(2024, 1, 4, 12, 0, 0)),   // Thu (4)
        measurement(CHANNEL_A2, 50, utc(2024, 1, 8, 12, 0, 0)),   // Mon (1)
        measurement(CHANNEL_A1, 10, utc(2024, 1, 5, 12, 0, 0)),   // Fri (5)
    ];
    let last_30_days = promenade_service(measurements)
        .detail_graphs_timeframe(
            station_vo::Id(Uuid::from_u128(STATION_1)),
            GraphTimeframe::Last30Days,
            now,
        )
        .unwrap();

    let radar = &last_30_days.weekday_radar;
    let by_weekday: HashMap<_, _> = radar.iter().map(|w| (w.weekday, w.total)).collect();
    assert_eq!(by_weekday[&1], 50, "Monday");
    assert_eq!(by_weekday[&3], 120, "Wednesday");
    assert_eq!(by_weekday[&4], 30, "Thursday");
    assert_eq!(by_weekday[&5], 10, "Friday");
    assert_eq!(radar.len(), 4, "only weekdays with data");
}

#[test]
fn detail_previous_and_hour_radars_are_computed() {
    let now = detail_now();
    // Last complete day (Berlin) = 2024-01-10; the day before = 2024-01-09.
    let measurements = vec![
        // Current day: Wed 2024-01-10, 09:00 Berlin (08:00 UTC).
        measurement(CHANNEL_A1, 100, utc(2024, 1, 10, 8, 0, 0)),
        // Previous day: Tue 2024-01-09, 21:00 Berlin (20:00 UTC).
        measurement(CHANNEL_A2, 50, utc(2024, 1, 9, 20, 0, 0)),
    ];
    let day = promenade_service(measurements)
        .detail_graphs_timeframe(
            station_vo::Id(Uuid::from_u128(STATION_1)),
            GraphTimeframe::Day,
            now,
        )
        .unwrap();

    let current_weekdays: HashMap<_, _> = day
        .weekday_radar
        .iter()
        .map(|w| (w.weekday, w.total))
        .collect();
    assert_eq!(current_weekdays[&3], 100, "Wednesday (current day)");
    let previous_weekdays: HashMap<_, _> = day
        .weekday_radar_previous
        .iter()
        .map(|w| (w.weekday, w.total))
        .collect();
    assert_eq!(previous_weekdays[&2], 50, "Tuesday (previous day)");

    // The in-memory mock folds by UTC hour.
    let by_hour: HashMap<_, _> = day.hourly.iter().map(|h| (h.hour, h.total)).collect();
    assert_eq!(by_hour[&8], 100);
    let by_hour_previous: HashMap<_, _> = day
        .hourly_previous
        .iter()
        .map(|h| (h.hour, h.total))
        .collect();
    assert_eq!(by_hour_previous[&20], 50);

    let a = day
        .per_channel
        .iter()
        .find(|s| s.channel_id == Uuid::from_u128(CHANNEL_A1))
        .unwrap();
    assert_eq!(a.weekday_radar[0].weekday, 3);
    assert!(a.weekday_radar_previous.is_empty());
    assert_eq!(a.hourly[0].hour, 8);
    assert!(a.hourly_previous.is_empty());

    let b = day
        .per_channel
        .iter()
        .find(|s| s.channel_id == Uuid::from_u128(CHANNEL_A2))
        .unwrap();
    assert!(b.weekday_radar.is_empty());
    assert_eq!(b.weekday_radar_previous[0].weekday, 2);
    assert!(b.hourly.is_empty());
    assert_eq!(b.hourly_previous[0].hour, 20);
}

#[test]
fn detail_unknown_station_is_an_error() {
    let result = promenade_service(Vec::new())
        .detail_page(station_vo::Id(Uuid::from_u128(0x999)), detail_now());
    assert!(matches!(result, Err(DomainError::NotFound(_))));
}

#[test]
fn detail_station_without_channels_returns_empty_graphs() {
    let service = service(
        vec![station(STATION_1, "Promenade", None)],
        vec![],
        vec![],
        vec![],
    );
    let id = station_vo::Id(Uuid::from_u128(STATION_1));
    let page = service.detail_page(id, detail_now()).unwrap();
    assert!(page.channels.is_empty());
    let day = service
        .detail_graphs_timeframe(id, GraphTimeframe::Day, detail_now())
        .unwrap();
    assert!(day.current.is_empty());
    assert!(day.per_channel.is_empty());
    assert!(day.channel_pie.is_empty());
    let monthly = service.detail_monthly(id, detail_now()).unwrap();
    assert!(monthly.is_empty());
}

#[test]
fn detail_overview_stats_returns_all_time_total_and_metrics() {
    let now = detail_now();
    let measurements = vec![
        measurement(CHANNEL_A1, 100, utc(2024, 1, 10, 12, 0, 0)), // last day
        measurement(CHANNEL_A1, 20, utc(2023, 12, 20, 12, 0, 0)), // previous month
        measurement(CHANNEL_A1, 7, utc(2023, 6, 15, 12, 0, 0)),   // previous year
    ];
    let stats = promenade_service(measurements)
        .detail_overview_stats(station_vo::Id(Uuid::from_u128(STATION_1)), now)
        .unwrap();
    assert_eq!(stats.total_bikes, 127, "sum over the whole history");
    assert_eq!(stats.metrics.len(), 4);
    let day = stats
        .metrics
        .iter()
        .find(|window| window.key == MetricKey::LastDay)
        .unwrap();
    assert_eq!(day.current, 100);
}

#[test]
fn graph_timeframe_keys_roundtrip() {
    for timeframe in GraphTimeframe::ALL {
        assert_eq!(
            GraphTimeframe::from_key(timeframe.as_str()),
            Some(timeframe)
        );
    }
    assert_eq!(GraphTimeframe::Day.as_str(), "day");
    assert_eq!(GraphTimeframe::Week.as_str(), "week");
    assert_eq!(GraphTimeframe::Last30Days.as_str(), "last_30_days");
    assert_eq!(GraphTimeframe::Year.as_str(), "year");
    assert_eq!(GraphTimeframe::from_key("nope"), None);
}

// -----------------------------------------------------------------------
// station-summary page
// -----------------------------------------------------------------------

/// Fixed "now": 2024-01-11 12:00 UTC = 13:00 Berlin (CET).
fn summary_now() -> DateTime<Utc> {
    utc(2024, 1, 11, 12, 0, 0)
}

/// Default fixture: A (id 1) + B (id 2) inside the bounds, C outside.
fn default_summary_service(measurements: Vec<Measurement>) -> StationAnalyticsService {
    service(
        vec![
            station(STATION_1, "A", Some((51.96, 7.63))),
            station(STATION_B, "B", Some((51.94, 7.6))),
            station(STATION_C, "C", Some((50.0, 5.0))),
        ],
        vec![
            channel(CHANNEL_A1, STATION_1, "a1"),
            channel(CHANNEL_A2, STATION_1, "a2"),
            channel(CHANNEL_B1, STATION_B, "b1"),
            channel(CHANNEL_C1, STATION_C, "c1"),
        ],
        measurements,
        vec![],
    )
}

fn metric_of(summary: &StationsSummaryOverview, key: MetricKey) -> &MetricWindow {
    summary
        .metrics
        .iter()
        .find(|metric| metric.key == key)
        .expect("metric present")
}

#[test]
fn stations_summary_filters_by_bounds_and_counts_channels() {
    let service = default_summary_service(Vec::new());
    let page = service
        .stations_summary_page(bounds(), summary_now())
        .unwrap();

    assert_eq!(page.stations.len(), 2, "A and B are inside the bounds");
    let by_id: HashMap<_, _> = page
        .stations
        .iter()
        .map(|s| (s.id, s.channel_count))
        .collect();
    assert_eq!(by_id.get(&Uuid::from_u128(STATION_1)), Some(&2));
    assert_eq!(by_id.get(&Uuid::from_u128(STATION_B)), Some(&1));

    let overview = service
        .stations_summary_overview(bounds(), &[], summary_now())
        .unwrap();
    assert_eq!(overview.channel_count, 3, "all included channels");
    assert_eq!(overview.metrics.len(), 4);
    assert_eq!(
        overview.total_bikes, 0,
        "no measurements, no all-time total"
    );
}

#[test]
fn stations_summary_keeps_disabled_stations_in_the_list_but_excludes_them_from_aggregation() {
    let service = default_summary_service(Vec::new());
    let exclude = [station_vo::Id(Uuid::from_u128(STATION_1))];

    let page = service
        .stations_summary_page(bounds(), summary_now())
        .unwrap();
    // A is still rendered (so the map can gray it out) …
    assert_eq!(page.stations.len(), 2);
    assert!(
        page.stations
            .iter()
            .any(|s| s.id == Uuid::from_u128(STATION_1))
    );

    // … but its channels are excluded from the aggregation.
    let overview = service
        .stations_summary_overview(bounds(), &exclude, summary_now())
        .unwrap();
    assert_eq!(overview.channel_count, 1, "only station B's channel");
}

#[test]
fn stations_summary_aggregates_all_four_metrics_across_stations() {
    let measurements = vec![
        measurement(CHANNEL_A1, 10, utc(2024, 1, 10, 12, 0, 0)),
        measurement(CHANNEL_B1, 5, utc(2024, 1, 10, 13, 0, 0)),
        measurement(CHANNEL_A2, 3, utc(2024, 1, 4, 12, 0, 0)),
        measurement(CHANNEL_B1, 2, utc(2023, 12, 15, 12, 0, 0)),
        measurement(CHANNEL_A1, 50, utc(2023, 6, 15, 12, 0, 0)),
        measurement(CHANNEL_A2, 100, utc(2022, 6, 15, 12, 0, 0)),
    ];
    let overview = default_summary_service(measurements)
        .stations_summary_overview(bounds(), &[], summary_now())
        .unwrap();

    assert_eq!(metric_of(&overview, MetricKey::LastDay).current, 15);
    assert_eq!(metric_of(&overview, MetricKey::Last7Days).current, 18);
    assert_eq!(metric_of(&overview, MetricKey::LastMonth).current, 2);
    assert_eq!(metric_of(&overview, MetricKey::LastYear).current, 52);
}

#[test]
fn stations_summary_aggregates_per_station_graphs_and_station_pie() {
    let measurements = vec![
        measurement(CHANNEL_A1, 100, utc(2024, 1, 8, 12, 0, 0)),
        measurement(CHANNEL_A2, 20, utc(2024, 1, 9, 12, 0, 0)),
        measurement(CHANNEL_B1, 50, utc(2024, 1, 8, 13, 0, 0)),
    ];
    let week = default_summary_service(measurements)
        .stations_summary_graphs_timeframe(bounds(), &[], GraphTimeframe::Week, summary_now())
        .unwrap();

    assert_eq!(sum_buckets(&week.current), 170, "aggregate current week");
    let pie: HashMap<_, _> = week
        .station_pie
        .iter()
        .map(|total| (total.station_id, total.total))
        .collect();
    assert_eq!(pie.get(&Uuid::from_u128(STATION_1)), Some(&120));
    assert_eq!(pie.get(&Uuid::from_u128(STATION_B)), Some(&50));
    assert_eq!(week.per_station.len(), 2);
    assert_eq!(week.per_station[0].station_id, Uuid::from_u128(STATION_1));
    assert_eq!(sum_buckets(&week.per_station[0].current), 120);
    assert_eq!(week.per_station[1].station_id, Uuid::from_u128(STATION_B));
    assert_eq!(sum_buckets(&week.per_station[1].current), 50);
    let radar_total: i64 = week.weekday_radar.iter().map(|w| w.total).sum();
    assert_eq!(radar_total, 170);
}

#[test]
fn stations_summary_per_station_weekday_radar_folds_each_station_buckets() {
    let measurements = vec![
        measurement(CHANNEL_A1, 10, utc(2024, 1, 8, 12, 0, 0)), // Mon
        measurement(CHANNEL_A1, 30, utc(2024, 1, 10, 12, 0, 0)), // Wed
        measurement(CHANNEL_B1, 7, utc(2024, 1, 8, 12, 0, 0)),  // Mon
    ];
    let week = default_summary_service(measurements)
        .stations_summary_graphs_timeframe(bounds(), &[], GraphTimeframe::Week, summary_now())
        .unwrap();

    let a_radar = &week.per_station[0].weekday_radar;
    let by_weekday: HashMap<_, _> = a_radar.iter().map(|w| (w.weekday, w.total)).collect();
    assert_eq!(by_weekday.get(&1), Some(&10), "Monday");
    assert_eq!(by_weekday.get(&3), Some(&30), "Wednesday");
}

#[test]
fn stations_summary_computes_previous_and_hour_radars() {
    let measurements = vec![
        measurement(CHANNEL_A1, 100, utc(2024, 1, 10, 8, 0, 0)), // current week
        measurement(CHANNEL_B1, 50, utc(2024, 1, 2, 20, 0, 0)),  // previous week
    ];
    let week = default_summary_service(measurements)
        .stations_summary_graphs_timeframe(bounds(), &[], GraphTimeframe::Week, summary_now())
        .unwrap();

    let current_weekdays: HashMap<_, _> = week
        .weekday_radar
        .iter()
        .map(|w| (w.weekday, w.total))
        .collect();
    assert_eq!(current_weekdays[&3], 100, "Wednesday (current week)");
    let previous_weekdays: HashMap<_, _> = week
        .weekday_radar_previous
        .iter()
        .map(|w| (w.weekday, w.total))
        .collect();
    assert_eq!(previous_weekdays[&2], 50, "Tuesday (previous week)");

    let by_hour: HashMap<_, _> = week.hourly.iter().map(|h| (h.hour, h.total)).collect();
    assert_eq!(by_hour[&8], 100);
    let by_hour_previous: HashMap<_, _> = week
        .hourly_previous
        .iter()
        .map(|h| (h.hour, h.total))
        .collect();
    assert_eq!(by_hour_previous[&20], 50);

    let a = week
        .per_station
        .iter()
        .find(|s| s.station_id == Uuid::from_u128(STATION_1))
        .unwrap();
    assert_eq!(a.weekday_radar[0].weekday, 3);
    assert!(a.weekday_radar_previous.is_empty());
    assert_eq!(a.hourly[0].hour, 8);
    assert!(a.hourly_previous.is_empty());

    let b = week
        .per_station
        .iter()
        .find(|s| s.station_id == Uuid::from_u128(STATION_B))
        .unwrap();
    assert!(b.weekday_radar.is_empty());
    assert_eq!(b.weekday_radar_previous[0].weekday, 2);
    assert!(b.hourly.is_empty());
    assert_eq!(b.hourly_previous[0].hour, 20);
}

#[test]
fn stations_summary_computes_monthly_totals_over_the_union() {
    let measurements = vec![
        measurement(CHANNEL_A1, 100, utc(2024, 1, 10, 12, 0, 0)),
        measurement(CHANNEL_B1, 25, utc(2023, 12, 15, 12, 0, 0)),
    ];
    let service = default_summary_service(measurements);
    let monthly = service
        .stations_summary_monthly(bounds(), &[], summary_now())
        .unwrap();
    assert_eq!(
        monthly,
        vec![
            MonthTotal {
                year: 2023,
                month: 12,
                total: 25
            },
            MonthTotal {
                year: 2024,
                month: 1,
                total: 100
            },
        ]
    );
    let overview = service
        .stations_summary_overview(bounds(), &[], summary_now())
        .unwrap();
    assert_eq!(overview.total_bikes, 125);
}

#[test]
fn stations_summary_empty_bounds_returns_empty_stations_and_graphs() {
    let empty_bounds = GeoBounds {
        min_latitude: 55.0,
        min_longitude: 10.0,
        max_latitude: 56.0,
        max_longitude: 11.0,
    };
    let service = default_summary_service(Vec::new());

    let page = service
        .stations_summary_page(empty_bounds, summary_now())
        .unwrap();
    assert!(page.stations.is_empty());

    let overview = service
        .stations_summary_overview(empty_bounds, &[], summary_now())
        .unwrap();
    assert_eq!(overview.channel_count, 0);
    assert_eq!(overview.total_bikes, 0);
    assert!(
        overview
            .metrics
            .iter()
            .all(|m| m.current == 0 && m.previous == 0)
    );

    let week = service
        .stations_summary_graphs_timeframe(empty_bounds, &[], GraphTimeframe::Week, summary_now())
        .unwrap();
    assert!(week.current.is_empty());
    assert!(week.per_station.is_empty());

    let monthly = service
        .stations_summary_monthly(empty_bounds, &[], summary_now())
        .unwrap();
    assert!(monthly.is_empty());
}

#[test]
fn stations_summary_last_update_comes_from_the_newest_finished_job() {
    let older = finished_job(0x61, utc(2024, 1, 10, 8, 0, 0));
    let newer = finished_job(0x62, utc(2024, 1, 11, 8, 0, 0));
    let service = service(
        vec![station(STATION_1, "A", Some((51.96, 7.63)))],
        vec![channel(CHANNEL_A1, STATION_1, "a1")],
        vec![],
        vec![older, newer],
    );
    let page = service
        .stations_summary_page(bounds(), summary_now())
        .unwrap();
    assert_eq!(page.last_update, Some(utc(2024, 1, 11, 8, 0, 0)));
}

#[test]
fn stations_summary_propagates_invalid_timezone() {
    let mut bad = station(STATION_1, "A", Some((51.96, 7.63)));
    bad.timezone = station_vo::Timezone("Not/AZone".to_string());
    let service = service(
        vec![bad],
        vec![channel(CHANNEL_A1, STATION_1, "a1")],
        vec![],
        vec![],
    );
    let result = service.stations_summary_overview(bounds(), &[], summary_now());
    assert!(matches!(result, Err(DomainError::InvalidQuery(_))));
}

// -----------------------------------------------------------------------
// shared helpers
// -----------------------------------------------------------------------

#[test]
fn graph_windows_rejects_invalid_timezone_via_detail() {
    let mut bad = station(STATION_1, "Promenade", None);
    bad.timezone = station_vo::Timezone("Not/AZone".to_string());
    let service = service(
        vec![bad],
        vec![channel(CHANNEL_A1, STATION_1, "a1")],
        vec![],
        vec![],
    );
    let result = service.detail_graphs_timeframe(
        station_vo::Id(Uuid::from_u128(STATION_1)),
        GraphTimeframe::Week,
        detail_now(),
    );
    assert!(matches!(result, Err(DomainError::InvalidQuery(_))));
}
