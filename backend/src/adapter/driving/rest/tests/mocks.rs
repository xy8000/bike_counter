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
use crate::core::domain::error::DomainError;
use crate::core::domain::health::{HealthService, HealthStatus, ServiceHealthIndicator};
use crate::core::domain::jobs::job::{Job, JobStatus};
use crate::core::domain::jobs::repository_port::JobRepository;
use crate::core::domain::measurements::measurement::Measurement;
use crate::core::domain::measurements::measurement::value_objects as measurement_vo;
use crate::core::domain::measurements::repository_port::MeasurementRepository;

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
        measurements.sort_by(|a, b| b.timestamp.0.cmp(&a.timestamp.0));
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
        _bucket_seconds: i64,
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
        _bucket_seconds: i64,
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
        _from: DateTime<Utc>,
        _to: DateTime<Utc>,
        _channel_ids: &[measurement_vo::ChannelId],
        _resolution_seconds: Option<i64>,
    ) -> Result<Vec<crate::core::domain::measurements::repository_port::ChannelTotal>, DomainError>
    {
        Ok(Vec::new())
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

/// In-memory job repository standing in for the real database.
pub struct MockJobRepository {
    pub jobs: Vec<Job>,
}

impl MockJobRepository {
    pub fn new(jobs: Vec<Job>) -> Self {
        Self { jobs }
    }
}

impl JobRepository for MockJobRepository {
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

    fn find_by_id(&self, id: Uuid) -> Result<Option<Job>, DomainError> {
        Ok(self.jobs.iter().find(|job| job.id == id).cloned())
    }

    fn find_all(
        &self,
        job_type: Option<&str>,
        status: Option<JobStatus>,
    ) -> Result<Vec<Job>, DomainError> {
        let mut jobs = self.jobs.clone();
        if let Some(job_type) = job_type {
            jobs.retain(|job| job.job_type == job_type);
        }
        if let Some(status) = status {
            jobs.retain(|job| job.status == status);
        }
        Ok(jobs)
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
        messages.sort_by(|a, b| b.occurred_at.cmp(&a.occurred_at));
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
