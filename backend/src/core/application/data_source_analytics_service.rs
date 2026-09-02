//! Application service aggregating the per-data-source overview / detail read
//! models served by the BFF data-sources endpoints.

use std::sync::Arc;

use chrono::{DateTime, Datelike, Duration, TimeZone, Utc};
use chrono_tz::Tz;

use crate::core::domain::channels::repository_port::ChannelRepository;
use crate::core::domain::counting_stations::counting_station::value_objects as station_vo;
use crate::core::domain::counting_stations::repository_port::CountingStationRepository;
use crate::core::domain::data_source::data_source::value_objects::Id as DataSourceId;
use crate::core::domain::data_source::import_run_port::DataImportRunRepository;
use crate::core::domain::data_source::provider_message::ProviderMessageSeverity;
use crate::core::domain::data_source::provider_message_port::ProviderMessageStore;
use crate::core::domain::data_source::repository_port::DataSourceRepository;
use crate::core::domain::data_source_analytics::{
    DataSourceAnalyticsServicePort, DataSourceDetail, DataSourceOverview,
};
use crate::core::domain::error::DomainError;
use crate::core::domain::measurements::measurement::value_objects as measurement_vo;
use crate::core::domain::measurements::repository_port::MeasurementRepository;

/// Aggregates per-data-source facts for the BFF data-sources pages.
pub struct DataSourceAnalyticsService {
    data_source_repository: Arc<dyn DataSourceRepository + Send + Sync>,
    counting_station_repository: Arc<dyn CountingStationRepository + Send + Sync>,
    channel_repository: Arc<dyn ChannelRepository + Send + Sync>,
    measurement_repository: Arc<dyn MeasurementRepository + Send + Sync>,
    import_run_repository: Arc<dyn DataImportRunRepository + Send + Sync>,
    provider_message_store: Arc<dyn ProviderMessageStore + Send + Sync>,
}

impl DataSourceAnalyticsService {
    #[allow(clippy::too_many_arguments)]
    pub fn new(
        data_source_repository: Arc<dyn DataSourceRepository + Send + Sync>,
        counting_station_repository: Arc<dyn CountingStationRepository + Send + Sync>,
        channel_repository: Arc<dyn ChannelRepository + Send + Sync>,
        measurement_repository: Arc<dyn MeasurementRepository + Send + Sync>,
        import_run_repository: Arc<dyn DataImportRunRepository + Send + Sync>,
        provider_message_store: Arc<dyn ProviderMessageStore + Send + Sync>,
    ) -> Self {
        Self {
            data_source_repository,
            counting_station_repository,
            channel_repository,
            measurement_repository,
            import_run_repository,
            provider_message_store,
        }
    }

    pub fn overview(&self) -> Result<Vec<DataSourceOverview>, DomainError> {
        self.data_source_repository
            .find_all()?
            .into_iter()
            .map(|data_source| {
                let source_id = station_vo::DataSourceId(data_source.id.0);
                let station_count = self
                    .counting_station_repository
                    .count_by_data_source_id(source_id)?;
                let channel_count = self
                    .channel_repository
                    .channel_ids_by_data_source_id(source_id)?
                    .len();
                // The last per-source import run drives the status shown in the
                // list (succeeded / running / failed) and the "running for …"
                // duration while an import is in progress.
                let last_import = self
                    .import_run_repository
                    .latest_by_data_source(data_source.id)?;
                Ok(DataSourceOverview {
                    id: data_source.id.0,
                    name: data_source.name.0,
                    provider_type: data_source.provider_type.0,
                    last_updated_at: data_source.last_updated_at,
                    station_count,
                    channel_count,
                    logo_asset_id: data_source.logo_asset_id,
                    last_import,
                })
            })
            .collect()
    }

    pub fn detail(&self, id: DataSourceId) -> Result<DataSourceDetail, DomainError> {
        let data_source = self
            .data_source_repository
            .find_by_id(id)?
            .ok_or(DomainError::NotFound(id.0))?;
        let source_id = station_vo::DataSourceId(id.0);

        let stations = self
            .counting_station_repository
            .find_by_data_source_id(source_id)?;
        let channel_ids: Vec<measurement_vo::ChannelId> = self
            .channel_repository
            .channel_ids_by_data_source_id(source_id)?
            .into_iter()
            .map(|channel_id| measurement_vo::ChannelId(channel_id.0))
            .collect();

        // First/last measurement derived from the per-channel first/last lookups
        // (each is an index seek that stops at one row per channel, so large
        // sources such as Hamburg are cheap — never a full-history scan).
        let first_data_at = self
            .measurement_repository
            .earliest_by_channel(&channel_ids)?
            .into_iter()
            .map(|earliest| earliest.timestamp)
            .min();
        let last_data_at = self
            .measurement_repository
            .latest_by_channel(&channel_ids)?
            .into_iter()
            .map(|latest| latest.timestamp)
            .max();

        let timezone = stations
            .first()
            .map(|station| station.timezone.0.clone())
            .unwrap_or_else(|| "UTC".to_string());
        let now = Utc::now();
        // "Full current year coverage" needs to know that every local month of
        // the current year has at least one measurement. Instead of aggregating
        // the source's whole current-year history (slow for high-frequency
        // sources such as Hamburg), probe one cheap index seek per month window.
        let windows = current_year_month_windows(&timezone, now);
        let month_presence = self
            .measurement_repository
            .has_measurements_in_windows(source_id, &windows)?;

        let last_import = self.import_run_repository.latest_by_data_source(id)?;
        let (last_import_warnings, last_import_errors) = match &last_import {
            Some(run) => (
                self.provider_message_store.count_since(
                    id,
                    ProviderMessageSeverity::Warning,
                    run.started_at,
                )?,
                self.provider_message_store.count_since(
                    id,
                    ProviderMessageSeverity::Error,
                    run.started_at,
                )?,
            ),
            None => (0, 0),
        };

        Ok(DataSourceDetail {
            data_source,
            station_count: stations.len(),
            channel_count: channel_ids.len(),
            stations,
            first_data_at,
            last_data_at,
            has_historical: first_data_at.is_some_and(|first| now - first > Duration::days(365)),
            has_real_time: last_data_at.is_some_and(|last| now - last < Duration::hours(24)),
            has_full_current_year: !month_presence.is_empty()
                && month_presence.iter().all(|present| *present),
            last_import,
            last_import_warnings,
            last_import_errors,
        })
    }
}

/// UTC half-open windows for every local calendar month from January up to and
/// including the month containing `now`, in the source's timezone. The "full
/// current year coverage" badge then checks one cheap presence probe per window
/// instead of aggregating the source's whole current-year history.
fn current_year_month_windows(
    timezone: &str,
    now: DateTime<Utc>,
) -> Vec<(DateTime<Utc>, DateTime<Utc>)> {
    let tz: Tz = timezone.parse().unwrap_or(chrono_tz::UTC);
    let local = now.with_timezone(&tz);
    let year = local.year();
    let current_month = local.month() as u8;
    (1..=current_month)
        .map(|month| {
            let (end_year, end_month) = if month == 12 {
                (year + 1, 1)
            } else {
                (year, month + 1)
            };
            let start = tz
                .with_ymd_and_hms(year, month as u32, 1, 0, 0, 0)
                .unwrap()
                .with_timezone(&Utc);
            let end = tz
                .with_ymd_and_hms(end_year, end_month as u32, 1, 0, 0, 0)
                .unwrap()
                .with_timezone(&Utc);
            (start, end)
        })
        .collect()
}

impl DataSourceAnalyticsServicePort for DataSourceAnalyticsService {
    fn overview(&self) -> Result<Vec<DataSourceOverview>, DomainError> {
        self.overview()
    }

    fn detail(&self, id: DataSourceId) -> Result<DataSourceDetail, DomainError> {
        self.detail(id)
    }
}

#[cfg(test)]
mod tests {
    use std::collections::HashMap;
    use std::sync::{Arc, Mutex};

    use chrono::{DateTime, Datelike, Duration, Utc};
    use uuid::Uuid;

    use super::{DataSourceAnalyticsService, current_year_month_windows};
    use crate::core::domain::channels::channel::{Channel, value_objects as channel_vo};
    use crate::core::domain::channels::repository_port::ChannelRepository;
    use crate::core::domain::counting_stations::counting_station::CountingStation;
    use crate::core::domain::counting_stations::counting_station::value_objects as station_vo;
    use crate::core::domain::counting_stations::repository_port::CountingStationRepository;
    use crate::core::domain::data_source::data_source::DataSource;
    use crate::core::domain::data_source::data_source::value_objects::Id as DataSourceId;
    use crate::core::domain::data_source::import_run::DataImportRun;
    use crate::core::domain::data_source::import_run_port::DataImportRunRepository;
    use crate::core::domain::data_source::provider_message::ProviderMessageSeverity;
    use crate::core::domain::data_source::provider_message_port::ProviderMessageStore;
    use crate::core::domain::data_source::repository_port::DataSourceRepository;
    use crate::core::domain::error::DomainError;
    use crate::core::domain::measurements::measurement::Measurement;
    use crate::core::domain::measurements::measurement::value_objects as measurement_vo;
    use crate::core::domain::measurements::repository_port::{
        ChannelFirst, ChannelLatest, MeasurementRepository, MonthTotal,
    };

    // -- In-memory repositories ------------------------------------------------

    #[derive(Default)]
    struct MemoryDataSourceRepository {
        data_sources: Mutex<Vec<DataSource>>,
    }

    impl DataSourceRepository for MemoryDataSourceRepository {
        fn upsert(&self, data_source: DataSource) -> Result<(), DomainError> {
            self.data_sources.lock().unwrap().push(data_source);
            Ok(())
        }

        fn find_by_id(&self, id: DataSourceId) -> Result<Option<DataSource>, DomainError> {
            Ok(self
                .data_sources
                .lock()
                .unwrap()
                .iter()
                .find(|data_source| data_source.id == id)
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
            _id: DataSourceId,
            _timestamp: DateTime<Utc>,
        ) -> Result<(), DomainError> {
            Ok(())
        }

        fn clear_imported_until(&self, _id: DataSourceId) -> Result<(), DomainError> {
            Ok(())
        }

        fn update_last_updated(
            &self,
            _id: DataSourceId,
            _timestamp: DateTime<Utc>,
        ) -> Result<(), DomainError> {
            Ok(())
        }
    }

    #[derive(Default)]
    struct MemoryCountingStationRepository {
        stations: Mutex<Vec<CountingStation>>,
    }

    impl CountingStationRepository for MemoryCountingStationRepository {
        fn save(&self, station: CountingStation) -> Result<(), DomainError> {
            self.stations.lock().unwrap().push(station);
            Ok(())
        }

        fn find_by_id(&self, id: station_vo::Id) -> Result<CountingStation, DomainError> {
            self.stations
                .lock()
                .unwrap()
                .iter()
                .find(|station| station.id == id)
                .cloned()
                .ok_or(DomainError::NotFound(id.0))
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
            self.find_all()
        }

        fn update(&self, _station: CountingStation) -> Result<(), DomainError> {
            Ok(())
        }
    }

    #[derive(Default)]
    struct MemoryChannelRepository {
        channels: Mutex<Vec<Channel>>,
        /// The data source a channel belongs to (through its station). The mock
        /// stores an explicit mapping so `channel_ids_by_data_source_id` works.
        station_source: Mutex<HashMap<Uuid, Uuid>>,
    }

    impl MemoryChannelRepository {
        fn set_station_source(&self, station_id: Uuid, data_source_id: Uuid) {
            self.station_source
                .lock()
                .unwrap()
                .insert(station_id, data_source_id);
        }
    }

    impl ChannelRepository for MemoryChannelRepository {
        fn save(&self, channel: Channel) -> Result<(), DomainError> {
            self.channels.lock().unwrap().push(channel);
            Ok(())
        }

        fn find_by_id(&self, id: channel_vo::Id) -> Result<Channel, DomainError> {
            self.channels
                .lock()
                .unwrap()
                .iter()
                .find(|channel| channel.id == id)
                .cloned()
                .ok_or(DomainError::NotFound(id.0))
        }

        fn find_all(&self) -> Result<Vec<Channel>, DomainError> {
            Ok(self.channels.lock().unwrap().clone())
        }

        fn find_by_counting_station_id(
            &self,
            station_id: channel_vo::CountingStationId,
        ) -> Result<Vec<Channel>, DomainError> {
            Ok(self
                .channels
                .lock()
                .unwrap()
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
            counting_station_id: Option<channel_vo::CountingStationId>,
            _name: Option<&str>,
        ) -> Result<Vec<Channel>, DomainError> {
            Ok(match counting_station_id {
                Some(station_id) => self.find_by_counting_station_id(station_id)?,
                None => self.find_all()?,
            })
        }

        fn channel_ids_by_data_source_id(
            &self,
            data_source_id: station_vo::DataSourceId,
        ) -> Result<Vec<channel_vo::Id>, DomainError> {
            let station_source = self.station_source.lock().unwrap();
            Ok(self
                .channels
                .lock()
                .unwrap()
                .iter()
                .filter(|channel| {
                    station_source
                        .get(&channel.counting_station_id.0)
                        .is_some_and(|source| *source == data_source_id.0)
                })
                .map(|channel| channel.id)
                .collect())
        }
    }

    #[derive(Default)]
    struct MemoryMeasurementRepository {
        first: Mutex<Vec<ChannelFirst>>,
        latest: Mutex<Vec<ChannelLatest>>,
        /// One boolean per calendar-month window of the current year the detail
        /// service asks about (index 0 = January). Drives the coverage badge.
        windows_present: Mutex<Vec<bool>>,
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
            Ok(Vec::new())
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
            _from: DateTime<Utc>,
            _to: DateTime<Utc>,
            _channel_ids: &[measurement_vo::ChannelId],
            _resolution_seconds: Option<i64>,
        ) -> Result<i64, DomainError> {
            Ok(0)
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
            _resolution_seconds: Option<i64>,
        ) -> Result<
            Vec<crate::core::domain::measurements::repository_port::WeekdayTotal>,
            DomainError,
        > {
            Ok(Vec::new())
        }

        fn sum_hours(
            &self,
            _from: DateTime<Utc>,
            _to: DateTime<Utc>,
            _timezone: &str,
            _channel_ids: &[measurement_vo::ChannelId],
            _resolution_seconds: Option<i64>,
        ) -> Result<Vec<crate::core::domain::measurements::repository_port::HourTotal>, DomainError>
        {
            Ok(Vec::new())
        }

        fn sum_hours_by_channel(
            &self,
            _from: DateTime<Utc>,
            _to: DateTime<Utc>,
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
        ) -> Result<Vec<MonthTotal>, DomainError> {
            Ok(Vec::new())
        }

        fn earliest_by_channel(
            &self,
            _channel_ids: &[measurement_vo::ChannelId],
        ) -> Result<Vec<ChannelFirst>, DomainError> {
            Ok(self.first.lock().unwrap().clone())
        }

        fn latest_by_channel(
            &self,
            _channel_ids: &[measurement_vo::ChannelId],
        ) -> Result<Vec<ChannelLatest>, DomainError> {
            Ok(self.latest.lock().unwrap().clone())
        }

        fn has_measurements_in_windows(
            &self,
            _data_source_id: station_vo::DataSourceId,
            windows: &[(DateTime<Utc>, DateTime<Utc>)],
        ) -> Result<Vec<bool>, DomainError> {
            let stored = self.windows_present.lock().unwrap();
            Ok(windows
                .iter()
                .enumerate()
                .map(|(index, _window)| stored.get(index).copied().unwrap_or(false))
                .collect())
        }
    }

    #[derive(Default)]
    struct MemoryImportRunRepository {
        runs: Mutex<Vec<DataImportRun>>,
    }

    impl DataImportRunRepository for MemoryImportRunRepository {
        fn insert(&self, run: &DataImportRun) -> Result<(), DomainError> {
            self.runs.lock().unwrap().push(run.clone());
            Ok(())
        }

        fn finish(&self, _id: Uuid, _finished_at: DateTime<Utc>) -> Result<(), DomainError> {
            Ok(())
        }

        fn fail(
            &self,
            _id: Uuid,
            _finished_at: DateTime<Utc>,
            _message: &str,
        ) -> Result<(), DomainError> {
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

    #[derive(Default)]
    struct MemoryProviderMessageStore {
        warnings_since: Mutex<i64>,
        errors_since: Mutex<i64>,
    }

    impl ProviderMessageStore for MemoryProviderMessageStore {
        fn record(
            &self,
            _data_source_id: DataSourceId,
            _severity: ProviderMessageSeverity,
            _message: &str,
        ) -> Result<(), DomainError> {
            Ok(())
        }

        fn find_by_data_source(
            &self,
            _data_source_id: DataSourceId,
        ) -> Result<
            Vec<crate::core::domain::data_source::provider_message::ProviderMessage>,
            DomainError,
        > {
            Ok(Vec::new())
        }

        fn count_since(
            &self,
            _data_source_id: DataSourceId,
            severity: ProviderMessageSeverity,
            _since: DateTime<Utc>,
        ) -> Result<i64, DomainError> {
            Ok(match severity {
                ProviderMessageSeverity::Warning => *self.warnings_since.lock().unwrap(),
                ProviderMessageSeverity::Error => *self.errors_since.lock().unwrap(),
                _ => 0,
            })
        }
    }

    // -- Helpers ---------------------------------------------------------------

    fn source(_id: u128, name: &str) -> DataSource {
        DataSource::new(name.to_string(), "provider".to_string())
    }

    fn station(source_id: Uuid, id: u128, name: &str) -> CountingStation {
        CountingStation {
            id: station_vo::Id(Uuid::from_u128(id)),
            name: station_vo::Name(name.to_string()),
            description: station_vo::Description("desc".to_string()),
            external_datasource_id: None,
            data_source_id: Some(station_vo::DataSourceId(source_id)),
            coordinates: Some(station_vo::GeoCoordinates {
                latitude: 51.9,
                longitude: 7.6,
            }),
            timezone: station_vo::Timezone("Europe/Berlin".to_string()),
            image_asset_id: None,
            image_sha256: None,
            status: station_vo::Status::Active,
        }
    }

    fn channel(id: u128, station_id: Uuid) -> Channel {
        Channel {
            id: channel_vo::Id(Uuid::from_u128(id)),
            counting_station_id: channel_vo::CountingStationId(station_id),
            name: channel_vo::Name("channel".to_string()),
            description: channel_vo::Description("desc".to_string()),
            external_datasource_id: None,
        }
    }

    fn service(
        data_sources: MemoryDataSourceRepository,
        stations: MemoryCountingStationRepository,
        channels: MemoryChannelRepository,
        measurements: MemoryMeasurementRepository,
        runs: MemoryImportRunRepository,
        messages: MemoryProviderMessageStore,
    ) -> DataSourceAnalyticsService {
        DataSourceAnalyticsService::new(
            Arc::new(data_sources),
            Arc::new(stations),
            Arc::new(channels),
            Arc::new(measurements),
            Arc::new(runs),
            Arc::new(messages),
        )
    }

    // -- Tests -----------------------------------------------------------------

    #[test]
    fn overview_reports_counts_and_last_update_per_source() {
        let data_sources = MemoryDataSourceRepository::default();
        let source_id = {
            let mut ds = source(1, "Münster");
            ds.last_updated_at = Some(
                DateTime::parse_from_rfc3339("2024-06-01T12:00:00Z")
                    .unwrap()
                    .with_timezone(&Utc),
            );
            data_sources.upsert(ds.clone()).unwrap();
            ds.id.0
        };
        let stations = MemoryCountingStationRepository::default();
        stations.save(station(source_id, 0x10, "A")).unwrap();
        stations.save(station(source_id, 0x11, "B")).unwrap();
        let channels = MemoryChannelRepository::default();
        channels.set_station_source(Uuid::from_u128(0x10), source_id);
        channels.set_station_source(Uuid::from_u128(0x11), source_id);
        channels.save(channel(0x20, Uuid::from_u128(0x10))).unwrap();
        channels.save(channel(0x21, Uuid::from_u128(0x10))).unwrap();
        channels.save(channel(0x22, Uuid::from_u128(0x11))).unwrap();

        let service = service(
            data_sources,
            stations,
            channels,
            MemoryMeasurementRepository::default(),
            MemoryImportRunRepository::default(),
            MemoryProviderMessageStore::default(),
        );

        let overview = service.overview().unwrap();
        assert_eq!(overview.len(), 1);
        assert_eq!(overview[0].name, "Münster");
        assert_eq!(overview[0].station_count, 2);
        assert_eq!(overview[0].channel_count, 3);
        assert!(overview[0].last_updated_at.is_some());
    }

    #[test]
    fn detail_derives_first_last_and_real_time_badge() {
        let data_sources = MemoryDataSourceRepository::default();
        let ds = source(1, "Münster");
        let source_id = ds.id.0;
        data_sources.upsert(ds.clone()).unwrap();

        let stations = MemoryCountingStationRepository::default();
        stations.save(station(source_id, 0x10, "A")).unwrap();
        let channels = MemoryChannelRepository::default();
        channels.set_station_source(Uuid::from_u128(0x10), source_id);
        let channel_id = Uuid::from_u128(0x20);
        channels.save(channel(0x20, Uuid::from_u128(0x10))).unwrap();

        let now = Utc::now();
        let measurements = MemoryMeasurementRepository::default();
        measurements.first.lock().unwrap().push(ChannelFirst {
            channel_id,
            timestamp: now - Duration::days(600),
        });
        measurements.latest.lock().unwrap().push(ChannelLatest {
            channel_id,
            timestamp: now - Duration::hours(1),
        });
        // Every elapsed month of the current year is present in the store.
        let elapsed = now.with_timezone(&chrono_tz::Europe::Berlin).month() as usize;
        measurements
            .windows_present
            .lock()
            .unwrap()
            .extend(std::iter::repeat_n(true, elapsed));

        let service = service(
            data_sources,
            stations,
            channels,
            measurements,
            MemoryImportRunRepository::default(),
            MemoryProviderMessageStore::default(),
        );

        let detail = service.detail(ds.id).unwrap();
        assert_eq!(detail.station_count, 1);
        assert_eq!(detail.channel_count, 1);
        assert!(detail.has_historical);
        assert!(detail.has_real_time);
        assert!(detail.has_full_current_year);
        assert!(detail.first_data_at.is_some());
        assert!(detail.last_data_at.is_some());
        assert!(detail.last_import.is_none());
    }

    #[test]
    fn detail_reports_last_import_facts_and_counters() {
        let data_sources = MemoryDataSourceRepository::default();
        let ds = source(1, "Münster");
        data_sources.upsert(ds.clone()).unwrap();

        let started = DateTime::parse_from_rfc3339("2024-06-01T10:00:00Z")
            .unwrap()
            .with_timezone(&Utc);
        let runs = MemoryImportRunRepository::default();
        let mut run = DataImportRun::start(Uuid::new_v4(), ds.id, None, started);
        run.finished_at = Some(started + Duration::minutes(5));
        run.status = crate::core::domain::data_source::import_run::ImportRunStatus::Failed;
        run.failure_message = Some("provider unreachable".to_string());
        runs.insert(&run).unwrap();

        let messages = MemoryProviderMessageStore::default();
        *messages.warnings_since.lock().unwrap() = 2;
        *messages.errors_since.lock().unwrap() = 1;

        let service = service(
            data_sources,
            MemoryCountingStationRepository::default(),
            MemoryChannelRepository::default(),
            MemoryMeasurementRepository::default(),
            runs,
            messages,
        );

        let detail = service.detail(ds.id).unwrap();
        let last_import = detail.last_import.unwrap();
        assert!(last_import.failed());
        assert_eq!(
            last_import.failure_message.as_deref(),
            Some("provider unreachable")
        );
        assert_eq!(detail.last_import_warnings, 2);
        assert_eq!(detail.last_import_errors, 1);
    }

    #[test]
    fn detail_unknown_source_is_not_found() {
        let service = service(
            MemoryDataSourceRepository::default(),
            MemoryCountingStationRepository::default(),
            MemoryChannelRepository::default(),
            MemoryMeasurementRepository::default(),
            MemoryImportRunRepository::default(),
            MemoryProviderMessageStore::default(),
        );
        assert!(matches!(
            service.detail(DataSourceId(Uuid::from_u128(0x99))),
            Err(DomainError::NotFound(_))
        ));
    }

    #[test]
    fn current_year_windows_cover_every_elapsed_month() {
        let now = Utc::now();
        let windows = current_year_month_windows("Europe/Berlin", now);
        let elapsed = now.with_timezone(&chrono_tz::Europe::Berlin).month() as usize;
        assert_eq!(windows.len(), elapsed);
        // Windows are contiguous, start at January and end after `now`.
        for pair in windows.windows(2) {
            assert_eq!(pair[0].1, pair[1].0);
        }
        assert!(!windows.is_empty());
        let last_end = windows.last().map(|window| window.1).unwrap();
        assert!(now < last_end);
    }

    #[test]
    fn detail_reports_no_full_year_coverage_when_a_month_has_no_data() {
        let data_sources = MemoryDataSourceRepository::default();
        let ds = source(2, "Hamburg");
        let source_id = ds.id.0;
        data_sources.upsert(ds.clone()).unwrap();

        let stations = MemoryCountingStationRepository::default();
        stations.save(station(source_id, 0x30, "A")).unwrap();
        let channels = MemoryChannelRepository::default();
        channels.set_station_source(Uuid::from_u128(0x30), source_id);
        channels.save(channel(0x40, Uuid::from_u128(0x30))).unwrap();

        let now = Utc::now();
        let elapsed = now.with_timezone(&chrono_tz::Europe::Berlin).month() as usize;
        let measurements = MemoryMeasurementRepository::default();
        // January has no data, so not every elapsed month is covered.
        let mut presence = vec![true; elapsed];
        if elapsed > 0 {
            presence[0] = false;
        }
        measurements
            .windows_present
            .lock()
            .unwrap()
            .extend(presence);

        let service = service(
            data_sources,
            stations,
            channels,
            measurements,
            MemoryImportRunRepository::default(),
            MemoryProviderMessageStore::default(),
        );

        let detail = service.detail(ds.id).unwrap();
        assert!(!detail.has_full_current_year);
    }
}
