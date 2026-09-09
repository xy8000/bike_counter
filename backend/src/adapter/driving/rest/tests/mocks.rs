//! In-memory repositories that back the router in tests (no database required).

use std::collections::{BTreeMap, HashMap};
use std::future::Future;
use std::pin::Pin;
use std::sync::{Arc, Mutex};

use chrono::{DateTime, Datelike, Utc};
use futures::Stream;
use uuid::Uuid;

use crate::adapter::driving::rest::tests::fixtures::{
    data_source_a, sample_channel_repository, sample_counting_station_repository,
    sample_job_repository, sample_measurement_repository,
};
use crate::core::application::channel_service::ChannelService;
use crate::core::application::counting_station_service::CountingStationService;
use crate::core::application::data_source_service::DataSourceService;
use crate::core::application::job_service::JobService;
use crate::core::application::measurement_service::MeasurementService;
use crate::core::application::opendata_service::OpenDataService;
use crate::core::application::persistent_state_service::PersistentStateService;
use crate::core::application::provider_message_service::ProviderMessageService;
use crate::core::application::station_analytics::StationAnalyticsService;
use crate::core::domain::assets::asset::value_objects::{
    AssetId, ByteSize, ContentType, ObjectKey, Sha256,
};
use crate::core::domain::assets::asset::{Asset, AssetOrigin, BuiltinImage};
use crate::core::domain::assets::asset_storage_port::{
    AssetObjectInfo, AssetObjectStream, AssetStorage,
};
use crate::core::domain::assets::service_port::AssetServicePort;
use crate::core::domain::channels::channel::Channel;
use crate::core::domain::channels::channel::value_objects as channel_vo;
use crate::core::domain::channels::repository_port::ChannelRepository;
use crate::core::domain::counting_stations::counting_station::CountingStation;
use crate::core::domain::counting_stations::counting_station::value_objects as station_vo;
use crate::core::domain::counting_stations::repository_port::CountingStationRepository;
use crate::core::domain::data_source::data_source::DataSource;
use crate::core::domain::data_source::data_source::value_objects as data_source_vo;
use crate::core::domain::data_source::persistent_state_port::PersistentStateStore;
use crate::core::domain::data_source::provider_message::{
    ProviderMessage, ProviderMessageSeverity,
};
use crate::core::domain::data_source::provider_message_port::ProviderMessageStore;
use crate::core::domain::data_source::repository_port::DataSourceRepository;
use crate::core::domain::data_source_analytics::{
    DataSourceAnalyticsServicePort, DataSourceDetail, DataSourceOverview,
};
use crate::core::domain::error::DomainError;
use crate::core::domain::health::{HealthService, HealthStatus, ServiceHealthIndicator};
use crate::core::domain::jobs::job::{Job, JobStatus};
use crate::core::domain::jobs::repository_port::JobRepository;
use crate::core::domain::measurements::measurement::Measurement;
use crate::core::domain::measurements::measurement::value_objects as measurement_vo;
use crate::core::domain::measurements::repository_port::MeasurementRepository;
use crate::core::domain::opendata::service_port::OpenDataServicePort;

pub struct MockCountingStationRepository {
    pub stations: Mutex<Vec<CountingStation>>,
}

impl MockCountingStationRepository {
    pub fn new(stations: Vec<CountingStation>) -> Self {
        Self {
            stations: Mutex::new(stations),
        }
    }
}

impl CountingStationRepository for MockCountingStationRepository {
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

    fn find_by_id(&self, id: station_vo::Id) -> Result<CountingStation, DomainError> {
        self.stations
            .lock()
            .unwrap()
            .iter()
            .find(|station| station.id.0 == id.0)
            .cloned()
            .ok_or(DomainError::NotFound(id.0))
    }

    fn find_all(&self) -> Result<Vec<CountingStation>, DomainError> {
        Ok(self.stations.lock().unwrap().clone())
    }

    fn find_by_external_datasource_id(
        &self,
        external_id: station_vo::ExternalDatasourceId,
    ) -> Result<Option<CountingStation>, DomainError> {
        Ok(self
            .stations
            .lock()
            .unwrap()
            .iter()
            .find(|station| {
                station
                    .external_datasource_id
                    .as_ref()
                    .map(|id| id.0 == external_id.0)
                    .unwrap_or(false)
            })
            .cloned())
    }

    fn find_filtered(&self, name: Option<&str>) -> Result<Vec<CountingStation>, DomainError> {
        let stations = self.stations.lock().unwrap();
        Ok(match name {
            Some(name) => stations
                .iter()
                .filter(|s| s.name.0.to_lowercase().contains(&name.to_lowercase()))
                .cloned()
                .collect(),
            None => stations.clone(),
        })
    }
}

pub struct MockChannelRepository {
    pub channels: Vec<Channel>,
}

impl ChannelRepository for MockChannelRepository {
    fn save(&self, _channel: Channel) -> Result<(), DomainError> {
        Ok(())
    }

    fn find_by_id(&self, id: channel_vo::Id) -> Result<Channel, DomainError> {
        self.channels
            .iter()
            .find(|channel| channel.id.0 == id.0)
            .cloned()
            .ok_or(DomainError::NotFound(id.0))
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
            .filter(|channel| channel.counting_station_id.0 == station_id.0)
            .cloned()
            .collect())
    }

    fn find_by_external_datasource_id(
        &self,
        external_id: channel_vo::ExternalDatasourceId,
    ) -> Result<Option<Channel>, DomainError> {
        Ok(self
            .channels
            .iter()
            .find(|channel| {
                channel
                    .external_datasource_id
                    .as_ref()
                    .map(|id| id.0 == external_id.0)
                    .unwrap_or(false)
            })
            .cloned())
    }

    fn find_filtered(
        &self,
        counting_station_id: Option<channel_vo::CountingStationId>,
        name: Option<&str>,
    ) -> Result<Vec<Channel>, DomainError> {
        Ok(self
            .channels
            .iter()
            .filter(|c| {
                counting_station_id.is_none_or(|id| c.counting_station_id.0 == id.0)
                    && name.is_none_or(|n| c.name.0.to_lowercase().contains(&n.to_lowercase()))
            })
            .cloned()
            .collect())
    }
}

pub struct MockMeasurementRepository {
    pub measurements: Vec<Measurement>,
}

impl MeasurementRepository for MockMeasurementRepository {
    fn save(&self, _measurement: Measurement) -> Result<(), DomainError> {
        Ok(())
    }

    fn save_batch(&self, _measurements: Vec<Measurement>) -> Result<u64, DomainError> {
        Ok(0)
    }

    fn find_by_id(&self, id: measurement_vo::Id) -> Result<Measurement, DomainError> {
        self.measurements
            .iter()
            .find(|measurement| measurement.id.0 == id.0)
            .cloned()
            .ok_or(DomainError::NotFound(id.0))
    }

    fn find_all(&self) -> Result<Vec<Measurement>, DomainError> {
        Ok(self.measurements.clone())
    }

    fn find_by_channel_id(
        &self,
        channel_id: measurement_vo::ChannelId,
    ) -> Result<Vec<Measurement>, DomainError> {
        Ok(self
            .measurements
            .iter()
            .filter(|measurement| measurement.channel_id.0 == channel_id.0)
            .cloned()
            .collect())
    }

    fn find_page(
        &self,
        channel_id: Option<measurement_vo::ChannelId>,
        offset: usize,
        limit: usize,
    ) -> Result<Vec<Measurement>, DomainError> {
        let mut measurements: Vec<Measurement> = self
            .measurements
            .iter()
            .filter(|measurement| channel_id.is_none_or(|id| measurement.channel_id.0 == id.0))
            .cloned()
            .collect();
        measurements.sort_by_key(|a| std::cmp::Reverse(a.timestamp.0));
        Ok(measurements.into_iter().skip(offset).take(limit).collect())
    }

    fn sum(
        &self,
        from: DateTime<Utc>,
        to: DateTime<Utc>,
        channel_ids: &[measurement_vo::ChannelId],
        _resolution_seconds: Option<i64>,
    ) -> Result<i64, DomainError> {
        Ok(self
            .measurements
            .iter()
            .filter(|m| m.timestamp.0 >= from && m.timestamp.0 <= to)
            .filter(|m| channel_ids.contains(&m.channel_id))
            .map(|m| m.value.0)
            .sum())
    }

    fn sum_buckets(
        &self,
        _from: DateTime<Utc>,
        _to: DateTime<Utc>,
        _granularity: crate::core::domain::measurements::repository_port::BucketGranularity,
        _origin: DateTime<Utc>,
        _timezone: &str,
        _channel_ids: &[measurement_vo::ChannelId],
        _resolution_seconds: Option<i64>,
    ) -> Result<Vec<crate::core::domain::measurements::repository_port::TimeBucket>, DomainError>
    {
        Ok(Vec::new())
    }

    fn sum_buckets_by_channel(
        &self,
        _from: DateTime<Utc>,
        _to: DateTime<Utc>,
        _granularity: crate::core::domain::measurements::repository_port::BucketGranularity,
        _origin: DateTime<Utc>,
        _timezone: &str,
        _channel_ids: &[measurement_vo::ChannelId],
        _resolution_seconds: Option<i64>,
    ) -> Result<Vec<crate::core::domain::measurements::repository_port::ChannelBucket>, DomainError>
    {
        Ok(Vec::new())
    }

    fn sum_weekdays(
        &self,
        _from: DateTime<Utc>,
        _to: DateTime<Utc>,
        _timezone: &str,
        _channel_ids: &[measurement_vo::ChannelId],
        _resolution_seconds: Option<i64>,
    ) -> Result<Vec<crate::core::domain::measurements::repository_port::WeekdayTotal>, DomainError>
    {
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
        from: DateTime<Utc>,
        to: DateTime<Utc>,
        channel_ids: &[measurement_vo::ChannelId],
        _resolution_seconds: Option<i64>,
    ) -> Result<Vec<crate::core::domain::measurements::repository_port::ChannelTotal>, DomainError>
    {
        let mut map: BTreeMap<Uuid, i64> = BTreeMap::new();
        for m in self
            .measurements
            .iter()
            .filter(|m| m.timestamp.0 >= from && m.timestamp.0 <= to)
            .filter(|m| channel_ids.iter().any(|id| id.0 == m.channel_id.0))
        {
            *map.entry(m.channel_id.0).or_insert(0) += m.value.0;
        }
        Ok(map
            .into_iter()
            .map(|(channel_id, total)| {
                crate::core::domain::measurements::repository_port::ChannelTotal {
                    channel_id,
                    total,
                }
            })
            .collect())
    }

    fn sum_by_month(
        &self,
        timezone: &str,
        channel_ids: &[measurement_vo::ChannelId],
        _resolution_seconds: Option<i64>,
    ) -> Result<Vec<crate::core::domain::measurements::repository_port::MonthTotal>, DomainError>
    {
        let tz: chrono_tz::Tz = timezone.parse().map_err(|_| {
            DomainError::InvalidQuery(format!("unknown IANA timezone '{timezone}'"))
        })?;
        let mut map: BTreeMap<(i32, u32), i64> = BTreeMap::new();
        for m in self
            .measurements
            .iter()
            .filter(|m| channel_ids.iter().any(|id| id.0 == m.channel_id.0))
        {
            let local = m.timestamp.0.with_timezone(&tz);
            *map.entry((local.year(), local.month())).or_insert(0) += m.value.0;
        }
        Ok(map
            .into_iter()
            .map(|((year, month), total)| {
                crate::core::domain::measurements::repository_port::MonthTotal {
                    year,
                    month: month as u8,
                    total,
                }
            })
            .collect())
    }

    fn earliest_by_channel(
        &self,
        channel_ids: &[measurement_vo::ChannelId],
    ) -> Result<Vec<crate::core::domain::measurements::repository_port::ChannelFirst>, DomainError>
    {
        let mut map: BTreeMap<Uuid, DateTime<Utc>> = BTreeMap::new();
        for m in self
            .measurements
            .iter()
            .filter(|m| channel_ids.iter().any(|id| id.0 == m.channel_id.0))
        {
            let entry = map.entry(m.channel_id.0).or_insert(m.timestamp.0);
            *entry = (*entry).min(m.timestamp.0);
        }
        Ok(map
            .into_iter()
            .map(|(channel_id, timestamp)| {
                crate::core::domain::measurements::repository_port::ChannelFirst {
                    channel_id,
                    timestamp,
                }
            })
            .collect())
    }
}

#[derive(Default)]
pub struct MockDataSourceRepository {
    pub data_sources: Vec<DataSource>,
}

impl DataSourceRepository for MockDataSourceRepository {
    fn upsert(&self, _data_source: DataSource) -> Result<(), DomainError> {
        Ok(())
    }

    fn find_by_id(&self, id: data_source_vo::Id) -> Result<Option<DataSource>, DomainError> {
        Ok(self
            .data_sources
            .iter()
            .find(|data_source| data_source.id.0 == id.0)
            .cloned())
    }

    fn find_by_name(&self, name: &str) -> Result<Option<DataSource>, DomainError> {
        Ok(self
            .data_sources
            .iter()
            .find(|data_source| data_source.name.0 == name)
            .cloned())
    }

    fn find_all(&self) -> Result<Vec<DataSource>, DomainError> {
        Ok(self.data_sources.clone())
    }

    fn delete(&self, _id: data_source_vo::Id) -> Result<(), DomainError> {
        Ok(())
    }

    fn update_imported_until(
        &self,
        _id: data_source_vo::Id,
        _timestamp: DateTime<Utc>,
    ) -> Result<(), DomainError> {
        Ok(())
    }

    fn clear_imported_until(&self, _id: data_source_vo::Id) -> Result<(), DomainError> {
        Ok(())
    }

    fn update_last_updated(
        &self,
        _id: data_source_vo::Id,
        _timestamp: DateTime<Utc>,
    ) -> Result<(), DomainError> {
        Ok(())
    }
}

/// A [`DataSourceAnalyticsServicePort`] mock standing in for the real
/// data-source analytics service in REST tests.
#[derive(Default)]
pub struct MockDataSourceAnalyticsService {
    pub data_source_repository: MockDataSourceRepository,
}

impl MockDataSourceAnalyticsService {
    pub fn new(data_source_repository: MockDataSourceRepository) -> Self {
        Self {
            data_source_repository,
        }
    }
}

impl DataSourceAnalyticsServicePort for MockDataSourceAnalyticsService {
    fn overview(&self) -> Result<Vec<DataSourceOverview>, DomainError> {
        self.data_source_repository
            .find_all()?
            .into_iter()
            .map(|data_source| {
                Ok(DataSourceOverview {
                    id: data_source.id.0,
                    name: data_source.name.0,
                    provider_type: data_source.provider_type.0,
                    last_updated_at: data_source.last_updated_at,
                    station_count: 1,
                    channel_count: 1,
                    logo_asset_id: None,
                    last_import: None,
                })
            })
            .collect()
    }

    fn detail(&self, id: data_source_vo::Id) -> Result<DataSourceDetail, DomainError> {
        let data_source = self
            .data_source_repository
            .find_by_id(id)?
            .ok_or(DomainError::NotFound(id.0))?;
        let station = CountingStation {
            id: station_vo::Id(Uuid::new_v4()),
            name: station_vo::Name(format!("{} station", data_source.name.0)),
            description: station_vo::Description("desc".to_string()),
            external_datasource_id: None,
            data_source_id: Some(station_vo::DataSourceId(id.0)),
            coordinates: Some(station_vo::GeoCoordinates {
                latitude: 51.9617,
                longitude: 7.6335,
            }),
            timezone: station_vo::Timezone("Europe/Berlin".to_string()),
            image_asset_id: None,
            image_sha256: None,
            status: station_vo::Status::Active,
        };
        Ok(DataSourceDetail {
            data_source,
            station_count: 1,
            channel_count: 1,
            stations: vec![station],
            first_data_at: None,
            last_data_at: None,
            has_historical: false,
            has_real_time: false,
            has_full_current_year: false,
            last_import: None,
            last_import_warnings: 0,
            last_import_errors: 0,
        })
    }
}

/// A [`DataSourceAnalyticsServicePort`] backed by the sample data-source repo.
pub fn sample_data_source_analytics_service()
-> Arc<dyn DataSourceAnalyticsServicePort + Send + Sync> {
    Arc::new(MockDataSourceAnalyticsService::new(
        MockDataSourceRepository {
            data_sources: vec![data_source_a()],
        },
    ))
}

/// A configurable health indicator standing in for a real downstream service.
pub struct MockServiceHealthIndicator {
    pub name: String,
    pub status: HealthStatus,
}

impl ServiceHealthIndicator for MockServiceHealthIndicator {
    fn name(&self) -> String {
        self.name.clone()
    }

    fn check(&self) -> HealthStatus {
        self.status.clone()
    }
}

/// A [`HealthService`] backed by a single mock PostgreSQL indicator, used by
/// the REST tests to exercise the readiness endpoint without a database.
pub fn mock_health_service(status: HealthStatus) -> Arc<HealthService> {
    Arc::new(HealthService::new(vec![Arc::new(
        MockServiceHealthIndicator {
            name: "postgres".to_string(),
            status,
        },
    )]))
}

/// In-memory job repository standing in for the real database. Mutex-backed so
/// the status-transition methods mutate state (the REST cancel tests observe
/// the resulting status through `JobService`).
pub struct MockJobRepository {
    jobs: Mutex<Vec<Job>>,
}

impl MockJobRepository {
    pub fn new(jobs: Vec<Job>) -> Self {
        Self {
            jobs: Mutex::new(jobs),
        }
    }
}

impl JobRepository for MockJobRepository {
    fn insert(&self, job: Job) -> Result<(), DomainError> {
        self.jobs.lock().unwrap().push(job);
        Ok(())
    }

    fn acquire(
        &self,
        _job_type: &str,
        _instance_id: Uuid,
        _lock_until: DateTime<Utc>,
    ) -> Result<bool, DomainError> {
        Ok(true)
    }

    fn release(&self, _job_type: &str, _instance_id: Uuid) -> Result<(), DomainError> {
        Ok(())
    }

    fn heartbeat(
        &self,
        id: Uuid,
        _job_type: &str,
        _instance_id: Uuid,
        _at: DateTime<Utc>,
        _lock_until: DateTime<Utc>,
    ) -> Result<JobStatus, DomainError> {
        self.find_by_id(id)?
            .map(|job| job.status)
            .ok_or(DomainError::NotFound(id))
    }

    fn set_finished(&self, id: Uuid, finished_at: DateTime<Utc>) -> Result<(), DomainError> {
        let mut jobs = self.jobs.lock().unwrap();
        if let Some(job) = jobs.iter_mut().find(|job| job.id == id)
            && job.status == JobStatus::Running
        {
            job.status = JobStatus::Finished;
            job.finished_at = Some(finished_at);
            return Ok(());
        }
        Err(DomainError::InvalidQuery("not running".to_string()))
    }

    fn set_failed(
        &self,
        id: Uuid,
        finished_at: DateTime<Utc>,
        message: &str,
    ) -> Result<(), DomainError> {
        let mut jobs = self.jobs.lock().unwrap();
        if let Some(job) = jobs.iter_mut().find(|job| job.id == id)
            && job.status == JobStatus::Running
        {
            job.status = JobStatus::Failed;
            job.finished_at = Some(finished_at);
            job.failure_message = Some(message.to_string());
            return Ok(());
        }
        Err(DomainError::InvalidQuery("not running".to_string()))
    }

    fn request_cancellation(&self, id: Uuid) -> Result<(), DomainError> {
        let mut jobs = self.jobs.lock().unwrap();
        if let Some(job) = jobs.iter_mut().find(|job| job.id == id)
            && job.status == JobStatus::Running
        {
            job.status = JobStatus::CancellationRequested;
            return Ok(());
        }
        Err(DomainError::InvalidQuery("not running".to_string()))
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
            return Ok(());
        }
        Err(DomainError::InvalidQuery("not cancellable".to_string()))
    }

    fn update_metadata(
        &self,
        _id: Uuid,
        _key: &str,
        _value: serde_json::Value,
    ) -> Result<(), DomainError> {
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
        job_type: Option<&str>,
        status: Option<JobStatus>,
    ) -> Result<Vec<Job>, DomainError> {
        let mut jobs = self.jobs.lock().unwrap().clone();
        if let Some(job_type) = job_type {
            jobs.retain(|job| job.job_type == job_type);
        }
        if let Some(status) = status {
            jobs.retain(|job| job.status == status);
        }
        Ok(jobs)
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

/// In-memory persistent state store standing in for the database.
#[derive(Default)]
pub struct MockPersistentStateStore {
    rows: Mutex<HashMap<data_source_vo::Id, HashMap<String, String>>>,
}

impl PersistentStateStore for MockPersistentStateStore {
    fn get(
        &self,
        data_source_id: data_source_vo::Id,
    ) -> Result<HashMap<String, String>, DomainError> {
        Ok(self
            .rows
            .lock()
            .unwrap()
            .get(&data_source_id)
            .cloned()
            .unwrap_or_default())
    }

    fn set(
        &self,
        data_source_id: data_source_vo::Id,
        key: &str,
        value: &str,
    ) -> Result<(), DomainError> {
        self.rows
            .lock()
            .unwrap()
            .entry(data_source_id)
            .or_default()
            .insert(key.to_string(), value.to_string());
        Ok(())
    }

    fn delete(&self, data_source_id: data_source_vo::Id, key: &str) -> Result<(), DomainError> {
        self.rows
            .lock()
            .unwrap()
            .get_mut(&data_source_id)
            .and_then(|rows| rows.remove(key));
        Ok(())
    }

    fn clear(&self, data_source_id: data_source_vo::Id) -> Result<(), DomainError> {
        self.rows.lock().unwrap().remove(&data_source_id);
        Ok(())
    }
}

/// A real [`PersistentStateService`] backed by an in-memory store and a data
/// source repository containing the sample data source.
pub fn sample_persistent_state_service() -> Arc<PersistentStateService> {
    let store = Arc::new(MockPersistentStateStore::default());
    let data_source_repository = Arc::new(MockDataSourceRepository {
        data_sources: vec![data_source_a()],
    });
    Arc::new(PersistentStateService::new(store, data_source_repository))
}

/// A [`CountingStationService`] backed by the sample counting-station repository.
pub fn sample_counting_station_service() -> Arc<CountingStationService> {
    Arc::new(CountingStationService::new(Arc::new(
        sample_counting_station_repository(),
    )))
}

/// A [`ChannelService`] backed by the sample channel repository.
pub fn sample_channel_service() -> Arc<ChannelService> {
    Arc::new(ChannelService::new(Arc::new(sample_channel_repository())))
}

/// A [`MeasurementService`] backed by the sample measurement repository.
pub fn sample_measurement_service() -> Arc<MeasurementService> {
    Arc::new(MeasurementService::new(Arc::new(
        sample_measurement_repository(),
    )))
}

/// A [`StationAnalyticsService`] backed by the sample counting-station, channel,
/// measurement and job repositories.
pub fn sample_station_analytics_service() -> Arc<StationAnalyticsService> {
    Arc::new(StationAnalyticsService::new(
        Arc::new(sample_counting_station_repository()),
        Arc::new(sample_channel_repository()),
        Arc::new(sample_measurement_repository()),
        Arc::new(sample_job_repository()),
        Arc::new(MockDataSourceRepository::default()),
    ))
}

/// A fixed built-in asset the asset mock services resolve.
fn mock_asset() -> Asset {
    Asset {
        id: AssetId(Uuid::from_u128(0xAAA)),
        object_key: ObjectKey("builtin/bike-icon-black-transparent.svg".to_string()),
        content_type: ContentType("image/svg+xml".to_string()),
        byte_size: ByteSize(3),
        sha256: Sha256("a".repeat(64)),
        origin: AssetOrigin::Builtin,
        created_at: Utc::now(),
        updated_at: Utc::now(),
    }
}

/// An [`AssetServicePort`] mock: always the built-in default asset.
pub struct MockAssetService {
    asset: Asset,
}

impl Default for MockAssetService {
    fn default() -> Self {
        Self {
            asset: mock_asset(),
        }
    }
}

impl AssetServicePort for MockAssetService {
    fn sync_builtin_images(&self, _builtin: &[BuiltinImage]) -> Result<(), DomainError> {
        Ok(())
    }
    fn default_asset(&self) -> Result<Asset, DomainError> {
        Ok(self.asset.clone())
    }
    fn store_provider_image(
        &self,
        _sha256: Sha256,
        _content_type: ContentType,
        _bytes: &[u8],
    ) -> Result<Asset, DomainError> {
        Ok(self.asset.clone())
    }
    fn find_by_id(&self, id: AssetId) -> Result<Option<Asset>, DomainError> {
        Ok((id == self.asset.id).then(|| self.asset.clone()))
    }
}

/// An [`AssetServicePort`] mock that always resolves the built-in default asset.
pub fn sample_asset_service() -> Arc<dyn AssetServicePort> {
    Arc::new(MockAssetService::default())
}

/// An [`AssetStorage`] mock that streams a fixed chunk of bytes.
pub struct MockAssetStorage;

impl AssetStorage for MockAssetStorage {
    fn ensure_bucket(&self) -> Result<(), DomainError> {
        Ok(())
    }
    fn put(
        &self,
        _object_key: &ObjectKey,
        _content_type: &ContentType,
        bytes: &[u8],
    ) -> Result<AssetObjectInfo, DomainError> {
        Ok(AssetObjectInfo {
            byte_size: bytes.len() as i64,
        })
    }
    fn list_object_keys(&self) -> Result<Vec<ObjectKey>, DomainError> {
        Ok(Vec::new())
    }
    fn delete(&self, _object_key: &ObjectKey) -> Result<(), DomainError> {
        Ok(())
    }
    fn get_stream(
        &self,
        _object_key: &ObjectKey,
    ) -> Pin<Box<dyn Future<Output = Result<AssetObjectStream, DomainError>> + Send + '_>> {
        Box::pin(async {
            let body: Box<dyn Stream<Item = Result<bytes::Bytes, std::io::Error>> + Send + Unpin> =
                Box::new(futures::stream::iter(vec![Ok(bytes::Bytes::from_static(
                    b"img",
                ))]));
            Ok(AssetObjectStream { body })
        })
    }
}

/// An [`AssetStorage`] mock standing in for MinIO in REST tests.
pub fn sample_asset_storage() -> Arc<dyn AssetStorage> {
    Arc::new(MockAssetStorage)
}

/// A [`DataSourceService`] backed by the given data-source repository.
pub fn sample_data_source_service(
    data_source_repository: MockDataSourceRepository,
) -> Arc<DataSourceService> {
    Arc::new(DataSourceService::new(Arc::new(data_source_repository)))
}

/// A [`JobService`] backed by the given job repository.
pub fn sample_job_service(job_repository: MockJobRepository) -> Arc<JobService> {
    Arc::new(JobService::new(Arc::new(job_repository)))
}

/// In-memory provider message store standing in for the database. `record`
/// generates the id and `occurred_at` locally, mirroring the database defaults.
#[derive(Default)]
pub struct MockProviderMessageStore {
    messages: Mutex<Vec<ProviderMessage>>,
}

impl MockProviderMessageStore {
    /// Seeds the store with pre-built messages (deterministic fixtures).
    pub fn seed(&self, messages: Vec<ProviderMessage>) {
        self.messages.lock().unwrap().extend(messages);
    }
}

impl ProviderMessageStore for MockProviderMessageStore {
    fn record(
        &self,
        data_source_id: data_source_vo::Id,
        severity: ProviderMessageSeverity,
        message: &str,
    ) -> Result<(), DomainError> {
        self.messages.lock().unwrap().push(ProviderMessage {
            id: Uuid::new_v4(),
            data_source_id,
            severity,
            message: message.to_string(),
            occurred_at: Utc::now(),
        });
        Ok(())
    }

    fn find_by_data_source(
        &self,
        data_source_id: data_source_vo::Id,
    ) -> Result<Vec<ProviderMessage>, DomainError> {
        let mut messages: Vec<ProviderMessage> = self
            .messages
            .lock()
            .unwrap()
            .iter()
            .filter(|message| message.data_source_id == data_source_id)
            .cloned()
            .collect();
        messages.sort_by_key(|a| std::cmp::Reverse(a.occurred_at));
        Ok(messages)
    }
}

/// A [`ProviderMessageService`] backed by an in-memory store and a data source
/// repository containing the sample data source.
pub fn sample_provider_message_service() -> Arc<ProviderMessageService> {
    sample_provider_message_service_with(Arc::new(MockProviderMessageStore::default()))
}

/// A [`ProviderMessageService`] backed by the given message store and a data
/// source repository containing the sample data source.
pub fn sample_provider_message_service_with(
    store: Arc<MockProviderMessageStore>,
) -> Arc<ProviderMessageService> {
    let data_source_repository = Arc::new(MockDataSourceRepository {
        data_sources: vec![data_source_a()],
    });
    Arc::new(ProviderMessageService::new(store, data_source_repository))
}

// -- OpenData test doubles -----------------------------------------------------

/// In-memory opendata-file registry standing in for the `opendata_files` table.
#[derive(Default)]
pub struct MockOpenDataFileRepository {
    files: Mutex<Vec<crate::core::domain::opendata::file::OpenDataFile>>,
}

impl MockOpenDataFileRepository {
    pub fn seed(&self, files: Vec<crate::core::domain::opendata::file::OpenDataFile>) {
        self.files.lock().unwrap().extend(files);
    }
}

impl crate::core::domain::opendata::file_repository_port::OpenDataFileRepository
    for MockOpenDataFileRepository
{
    fn insert(
        &self,
        file: &crate::core::domain::opendata::file::OpenDataFile,
    ) -> Result<(), DomainError> {
        self.files.lock().unwrap().push(file.clone());
        Ok(())
    }

    fn find_by_object_key(
        &self,
        object_key: &str,
    ) -> Result<Option<crate::core::domain::opendata::file::OpenDataFile>, DomainError> {
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
        granularity: crate::core::domain::opendata::file::Granularity,
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
        periods.sort();
        periods.dedup();
        Ok(periods)
    }

    fn find_by_period(
        &self,
        granularity: crate::core::domain::opendata::file::Granularity,
        period: &str,
        station_id: Option<Uuid>,
    ) -> Result<Vec<crate::core::domain::opendata::file::OpenDataFile>, DomainError> {
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
        granularity: crate::core::domain::opendata::file::Granularity,
        station_id: Option<Uuid>,
    ) -> Result<Option<String>, DomainError> {
        Ok(self
            .list_periods(granularity, station_id)?
            .into_iter()
            .next_back())
    }
}

/// An [`OpenDataServicePort`] backed by an empty in-memory registry.
pub fn sample_opendata_service() -> Arc<dyn OpenDataServicePort> {
    Arc::new(OpenDataService::new(Arc::new(
        MockOpenDataFileRepository::default(),
    )))
}

/// In-memory object storage keyed by object key (stands in for the opendata
/// MinIO bucket). `put` stores the bytes; `get_stream` streams them back.
#[derive(Default)]
pub struct MockObjectStorage {
    objects: Mutex<HashMap<String, Vec<u8>>>,
}

impl MockObjectStorage {
    /// Seeds (or overwrites) the stored bytes of one object key.
    pub fn put_bytes(&self, object_key: &str, bytes: Vec<u8>) {
        self.objects
            .lock()
            .unwrap()
            .insert(object_key.to_string(), bytes);
    }

    /// The stored bytes of one object key.
    pub fn bytes(&self, object_key: &str) -> Option<Vec<u8>> {
        self.objects.lock().unwrap().get(object_key).cloned()
    }
}

impl AssetStorage for MockObjectStorage {
    fn ensure_bucket(&self) -> Result<(), DomainError> {
        Ok(())
    }
    fn put(
        &self,
        object_key: &ObjectKey,
        _content_type: &ContentType,
        bytes: &[u8],
    ) -> Result<AssetObjectInfo, DomainError> {
        self.put_bytes(&object_key.0, bytes.to_vec());
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
    fn delete(&self, object_key: &ObjectKey) -> Result<(), DomainError> {
        self.objects.lock().unwrap().remove(&object_key.0);
        Ok(())
    }
    fn get_stream(
        &self,
        object_key: &ObjectKey,
    ) -> Pin<Box<dyn Future<Output = Result<AssetObjectStream, DomainError>> + Send + '_>> {
        let bytes = self.bytes(&object_key.0).unwrap_or_default();
        Box::pin(async move {
            let body: Box<dyn Stream<Item = Result<bytes::Bytes, std::io::Error>> + Send + Unpin> =
                Box::new(futures::stream::iter(vec![Ok(bytes::Bytes::from(bytes))]));
            Ok(AssetObjectStream { body })
        })
    }
}

/// An [`AssetStorage`] double for the opendata bucket (empty by default).
pub fn sample_opendata_storage() -> Arc<dyn AssetStorage> {
    Arc::new(MockObjectStorage::default())
}
