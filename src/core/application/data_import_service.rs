//! Imports counting stations, channels and measurements from the configured
//! external data sources. The runtime trigger (CLI / scheduling) is a separate,
//! deferred feature; the capability itself is built and tested here.

use std::collections::HashMap;
use std::sync::Arc;

use chrono::{DateTime, Utc};
use uuid::Uuid;

use crate::core::domain::channels::channel::Channel;
use crate::core::domain::channels::channel::value_objects as channel_vo;
use crate::core::domain::channels::repository_port::ChannelRepository;
use crate::core::domain::configuration::configuration::value_objects::DataSourceConfiguration;
use crate::core::domain::counting_stations::counting_station::CountingStation;
use crate::core::domain::counting_stations::counting_station::value_objects as station_vo;
use crate::core::domain::counting_stations::repository_port::CountingStationRepository;
use crate::core::domain::data_source::data_source::value_objects::Id as DataSourceId;
use crate::core::domain::data_source::provider_port::{
    DataProvider, MeasurementQuery, MeasurementRecord,
};
use crate::core::domain::error::DomainError;
use crate::core::domain::measurements::measurement::Measurement;
use crate::core::domain::measurements::measurement::value_objects as measurement_vo;
use crate::core::domain::measurements::repository_port::MeasurementRepository;

/// A configured data source together with its built provider.
#[derive(Clone)]
pub struct DataSourceRuntime {
    pub configuration: DataSourceConfiguration,
    /// Deterministic id derived from the data source name.
    pub data_source_id: DataSourceId,
    pub provider: Arc<dyn DataProvider>,
}

/// Counts of what was imported in a single run.
#[derive(Debug, Default, Clone, Copy, PartialEq, Eq)]
pub struct ImportSummary {
    pub data_sources: usize,
    pub counting_stations: usize,
    pub channels: usize,
    pub measurements: usize,
}

/// Result of incrementally updating a single data source.
#[derive(Debug, Default, Clone, Copy, PartialEq, Eq)]
pub struct DataSourceUpdate {
    pub processed_measurements: usize,
    /// Timestamp of the last processed measurement (the cursor to advance
    /// `data_sources.last_updated_at` to).
    pub last_measurement_timestamp: Option<DateTime<Utc>>,
}

pub struct DataImportService {
    counting_station_repository: Arc<dyn CountingStationRepository + Send + Sync>,
    channel_repository: Arc<dyn ChannelRepository + Send + Sync>,
    measurement_repository: Arc<dyn MeasurementRepository + Send + Sync>,
    runtimes: Vec<DataSourceRuntime>,
}

impl DataImportService {
    pub fn new(
        counting_station_repository: Arc<dyn CountingStationRepository + Send + Sync>,
        channel_repository: Arc<dyn ChannelRepository + Send + Sync>,
        measurement_repository: Arc<dyn MeasurementRepository + Send + Sync>,
        runtimes: Vec<DataSourceRuntime>,
    ) -> Self {
        Self {
            counting_station_repository,
            channel_repository,
            measurement_repository,
            runtimes,
        }
    }

    /// Imports all data sources between `from` and `to` (both optional).
    pub fn import(
        &self,
        from: Option<DateTime<Utc>>,
        to: Option<DateTime<Utc>>,
    ) -> Result<ImportSummary, DomainError> {
        let mut summary = ImportSummary {
            data_sources: self.runtimes.len(),
            ..ImportSummary::default()
        };

        for runtime in &self.runtimes {
            let station_ids = self.sync_counting_stations(runtime, &mut summary)?;
            let channels = self.sync_channels(runtime, &station_ids, &mut summary)?;
            for channel in &channels {
                self.import_measurements(runtime, channel, from, to, &mut summary)?;
            }
        }

        Ok(summary)
    }

    /// Saves new counting stations and returns a map of `external_id -> station
    /// UUID` so channels can be linked to their station. The core owns all
    /// entity identity: every new station gets a fresh `Uuid::new_v4()`.
    fn sync_counting_stations(
        &self,
        runtime: &DataSourceRuntime,
        summary: &mut ImportSummary,
    ) -> Result<HashMap<String, Uuid>, DomainError> {
        let stations = runtime
            .provider
            .get_all_counting_stations()
            .map_err(DomainError::from)?;

        let mut external_to_id = HashMap::with_capacity(stations.len());
        for record in stations {
            let external_id = station_vo::ExternalDatasourceId(record.external_id.clone());
            let station = match self
                .counting_station_repository
                .find_by_external_datasource_id(external_id.clone())?
            {
                Some(existing) => existing,
                None => {
                    let station = CountingStation {
                        id: station_vo::Id(Uuid::new_v4()),
                        name: station_vo::Name(record.name),
                        description: station_vo::Description(record.description),
                        external_datasource_id: Some(external_id),
                        data_source_id: Some(station_vo::DataSourceId(runtime.data_source_id.0)),
                    };
                    self.counting_station_repository.save(station.clone())?;
                    summary.counting_stations += 1;
                    station
                }
            };
            external_to_id.insert(record.external_id, station.id.0);
        }

        Ok(external_to_id)
    }

    /// Saves new channels and returns every channel (new + existing) so the
    /// caller can page through its measurements. Each new channel's
    /// `counting_station_id` is resolved from the station map built by
    /// [`Self::sync_counting_stations`].
    fn sync_channels(
        &self,
        runtime: &DataSourceRuntime,
        station_ids: &HashMap<String, Uuid>,
        summary: &mut ImportSummary,
    ) -> Result<Vec<Channel>, DomainError> {
        let channels = runtime
            .provider
            .get_all_channels()
            .map_err(DomainError::from)?;

        let mut result = Vec::with_capacity(channels.len());
        for record in channels {
            let external_id = channel_vo::ExternalDatasourceId(record.external_id.clone());
            let existing = self
                .channel_repository
                .find_by_external_datasource_id(external_id.clone())?;

            match existing {
                Some(existing) => result.push(existing),
                None => {
                    let counting_station_id = *station_ids
                        .get(&record.counting_station_external_id)
                        .ok_or_else(|| {
                            DomainError::InvalidQuery(format!(
                                "channel '{}' references unknown station '{}'",
                                record.external_id, record.counting_station_external_id
                            ))
                        })?;
                    let channel = Channel {
                        id: channel_vo::Id(Uuid::new_v4()),
                        counting_station_id: channel_vo::CountingStationId(counting_station_id),
                        name: channel_vo::Name(record.name),
                        description: channel_vo::Description(record.description),
                        external_datasource_id: Some(external_id),
                    };
                    self.channel_repository.save(channel.clone())?;
                    summary.channels += 1;
                    result.push(channel);
                }
            }
        }

        Ok(result)
    }

    fn import_measurements(
        &self,
        runtime: &DataSourceRuntime,
        channel: &Channel,
        from: Option<DateTime<Utc>>,
        to: Option<DateTime<Utc>>,
        summary: &mut ImportSummary,
    ) -> Result<(), DomainError> {
        let max_batch_size = runtime.provider.max_measurement_batch_size();
        let mut current_from = from;

        loop {
            let mut query = MeasurementQuery::for_channel(channel.clone(), max_batch_size);
            if let Some(from) = current_from {
                query = query.with_start(from);
            }
            if let Some(to) = to {
                query = query.with_end(to);
            }

            let batch = runtime
                .provider
                .get_measurements(query)
                .map_err(DomainError::from)?;

            summary.measurements += batch.measurements.len();
            let measurements = to_measurements(batch.measurements, channel.id.0);
            self.measurement_repository.save_batch(measurements)?;

            // Page while either limit was reached (row-count or time window);
            // use the last measurement datetime as the next start. Guard
            // against a missing last datetime to avoid an endless loop.
            if batch.last_measurement_datetime.is_some()
                && (batch.batch_size_limit_reached || batch.timeframe_limit_reached)
            {
                current_from = batch.last_measurement_datetime;
            } else {
                break;
            }
        }

        Ok(())
    }

    /// Incrementally updates a single data source in **strict order**:
    /// counting stations first, then channels, then measurements (paged from
    /// `from`, the data source's `last_updated_at`). `on_batch` is invoked with
    /// the running processed-measurement count after each persisted batch, so
    /// the caller can record progress (e.g. `processed_measurements` metadata).
    ///
    /// Returns how many measurements were processed and the timestamp of the
    /// last processed measurement (the cursor for the next run).
    pub fn update_data_source(
        &self,
        runtime: &DataSourceRuntime,
        from: Option<DateTime<Utc>>,
        on_batch: impl Fn(usize) -> Result<(), DomainError>,
    ) -> Result<DataSourceUpdate, DomainError> {
        let station_ids = self.sync_counting_stations(runtime, &mut ImportSummary::default())?;
        let channels = self.sync_channels(runtime, &station_ids, &mut ImportSummary::default())?;

        let mut processed = 0usize;
        let mut last_measurement_timestamp: Option<DateTime<Utc>> = None;

        for channel in &channels {
            let max_batch_size = runtime.provider.max_measurement_batch_size();
            let mut current_from = from;

            loop {
                let mut query = MeasurementQuery::for_channel(channel.clone(), max_batch_size);
                if let Some(from) = current_from {
                    query = query.with_start(from);
                }
                let batch = runtime
                    .provider
                    .get_measurements(query)
                    .map_err(DomainError::from)?;

                processed += batch.measurements.len();
                let measurements = to_measurements(batch.measurements, channel.id.0);
                self.measurement_repository.save_batch(measurements)?;
                on_batch(processed)?;

                match (
                    batch.last_measurement_datetime,
                    batch.batch_size_limit_reached || batch.timeframe_limit_reached,
                ) {
                    (Some(last), true) => {
                        current_from = Some(last);
                        last_measurement_timestamp = Some(last);
                    }
                    (Some(last), false) => {
                        last_measurement_timestamp = Some(last);
                        break;
                    }
                    (None, _) => break,
                }
            }
        }

        Ok(DataSourceUpdate {
            processed_measurements: processed,
            last_measurement_timestamp,
        })
    }
}

/// Converts provider measurement records into persisted entities: the core
/// generates a fresh UUID per measurement and attaches the channel id.
fn to_measurements(records: Vec<MeasurementRecord>, channel_id: Uuid) -> Vec<Measurement> {
    records
        .into_iter()
        .map(|record| Measurement {
            id: measurement_vo::Id(Uuid::new_v4()),
            channel_id: measurement_vo::ChannelId(channel_id),
            value: measurement_vo::Value(record.value),
            timestamp: measurement_vo::Timestamp(record.timestamp),
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use std::collections::{HashMap, VecDeque};
    use std::sync::{Arc, Mutex};

    use super::*;
    use crate::core::domain::channels::channel::value_objects as channel_vo;
    use crate::core::domain::configuration::configuration::value_objects::DataProviderConfiguration;
    use crate::core::domain::counting_stations::counting_station::CountingStation;
    use crate::core::domain::data_source::data_source::DataSource;
    use crate::core::domain::data_source::provider_port::{
        ChannelRecord, CountingStationRecord, MeasurementBatch, MeasurementRecord, ProviderError,
    };
    use crate::core::domain::health::HealthStatus;
    use crate::core::domain::measurements::measurement::Measurement;
    use crate::core::domain::measurements::measurement::value_objects as measurement_vo;

    fn data_source_config(name: &str, provider_type: &str) -> DataSourceConfiguration {
        DataSourceConfiguration::new(
            name.to_string(),
            DataProviderConfiguration::new(provider_type.to_string(), HashMap::new()).unwrap(),
        )
        .unwrap()
    }

    fn station(external_id: &str) -> CountingStation {
        CountingStation {
            id: station_vo::Id(Uuid::new_v4()),
            name: station_vo::Name(format!("Station {external_id}")),
            description: station_vo::Description("desc".to_string()),
            external_datasource_id: Some(station_vo::ExternalDatasourceId(external_id.to_string())),
            data_source_id: None,
        }
    }

    fn channel(external_id: &str) -> Channel {
        Channel {
            id: channel_vo::Id(Uuid::new_v4()),
            counting_station_id: channel_vo::CountingStationId(Uuid::new_v4()),
            name: channel_vo::Name(format!("Channel {external_id}")),
            description: channel_vo::Description("desc".to_string()),
            external_datasource_id: Some(channel_vo::ExternalDatasourceId(external_id.to_string())),
        }
    }

    fn station_record(external_id: &str) -> CountingStationRecord {
        CountingStationRecord {
            external_id: external_id.to_string(),
            name: format!("Station {external_id}"),
            description: "desc".to_string(),
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

    fn timestamp(rfc3339: &str) -> DateTime<Utc> {
        DateTime::parse_from_rfc3339(rfc3339)
            .expect("valid timestamp")
            .with_timezone(&Utc)
    }

    /// A provider that serves fixed external-id records and a queue of
    /// measurement pages. Every measurement query is recorded so tests can
    /// verify paging.
    struct MockProvider {
        stations: Vec<CountingStationRecord>,
        channels: Vec<ChannelRecord>,
        measurement_pages: Mutex<VecDeque<MeasurementBatch>>,
        recorded_queries: Mutex<Vec<MeasurementQuery>>,
        batch_size: usize,
    }

    impl DataProvider for MockProvider {
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
            query: MeasurementQuery,
        ) -> Result<MeasurementBatch, ProviderError> {
            self.recorded_queries.lock().unwrap().push(query);
            self.measurement_pages
                .lock()
                .unwrap()
                .pop_front()
                .ok_or_else(|| ProviderError::InvalidData("no more pages".to_string()))
        }

        fn max_measurement_batch_size(&self) -> usize {
            self.batch_size
        }
    }

    fn runtime(provider: Arc<MockProvider>) -> DataSourceRuntime {
        DataSourceRuntime {
            configuration: data_source_config("Münster", "münster_opendata_github_provider"),
            data_source_id: DataSourceId(DataSource::id_from_name("Münster")),
            provider,
        }
    }

    struct MockCountingStationRepository {
        stations: Mutex<Vec<CountingStation>>,
    }

    impl CountingStationRepository for MockCountingStationRepository {
        fn save(&self, station: CountingStation) -> Result<(), DomainError> {
            self.stations.lock().unwrap().push(station);
            Ok(())
        }

        fn find_by_id(&self, id: station_vo::Id) -> Result<CountingStation, DomainError> {
            self.stations
                .lock()
                .unwrap()
                .iter()
                .find(|s| s.id == id)
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
                .find(|s| {
                    s.external_datasource_id.as_ref().map(|e| e.0.as_str())
                        == Some(external_id.0.as_str())
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

    struct MockChannelRepository {
        channels: Mutex<Vec<Channel>>,
    }

    impl ChannelRepository for MockChannelRepository {
        fn save(&self, channel: Channel) -> Result<(), DomainError> {
            self.channels.lock().unwrap().push(channel);
            Ok(())
        }

        fn find_by_id(&self, id: channel_vo::Id) -> Result<Channel, DomainError> {
            self.channels
                .lock()
                .unwrap()
                .iter()
                .find(|c| c.id == id)
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
                .filter(|c| c.counting_station_id == station_id)
                .cloned()
                .collect())
        }

        fn find_by_external_datasource_id(
            &self,
            external_id: channel_vo::ExternalDatasourceId,
        ) -> Result<Option<Channel>, DomainError> {
            Ok(self
                .channels
                .lock()
                .unwrap()
                .iter()
                .find(|c| {
                    c.external_datasource_id.as_ref().map(|e| e.0.as_str())
                        == Some(external_id.0.as_str())
                })
                .cloned())
        }

        fn find_filtered(
            &self,
            counting_station_id: Option<channel_vo::CountingStationId>,
            name: Option<&str>,
        ) -> Result<Vec<Channel>, DomainError> {
            let channels = self.channels.lock().unwrap();
            Ok(channels
                .iter()
                .filter(|c| {
                    counting_station_id.is_none_or(|id| c.counting_station_id == id)
                        && name.is_none_or(|n| c.name.0.to_lowercase().contains(&n.to_lowercase()))
                })
                .cloned()
                .collect())
        }
    }

    struct MockMeasurementRepository {
        measurements: Mutex<Vec<Measurement>>,
    }

    impl MeasurementRepository for MockMeasurementRepository {
        fn save(&self, measurement: Measurement) -> Result<(), DomainError> {
            self.measurements.lock().unwrap().push(measurement);
            Ok(())
        }

        fn save_batch(&self, measurements: Vec<Measurement>) -> Result<(), DomainError> {
            self.measurements.lock().unwrap().extend(measurements);
            Ok(())
        }

        fn find_by_id(&self, id: measurement_vo::Id) -> Result<Measurement, DomainError> {
            self.measurements
                .lock()
                .unwrap()
                .iter()
                .find(|m| m.id.0 == id.0)
                .cloned()
                .ok_or(DomainError::NotFound(id.0))
        }

        fn find_all(&self) -> Result<Vec<Measurement>, DomainError> {
            Ok(self.measurements.lock().unwrap().clone())
        }

        fn find_by_channel_id(
            &self,
            channel_id: measurement_vo::ChannelId,
        ) -> Result<Vec<Measurement>, DomainError> {
            Ok(self
                .measurements
                .lock()
                .unwrap()
                .iter()
                .filter(|m| m.channel_id.0 == channel_id.0)
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
                .lock()
                .unwrap()
                .iter()
                .filter(|m| channel_id.is_none_or(|id| m.channel_id.0 == id.0))
                .cloned()
                .collect();
            measurements.sort_by(|a, b| b.timestamp.0.cmp(&a.timestamp.0));
            Ok(measurements.into_iter().skip(offset).take(limit).collect())
        }
    }

    #[test]
    fn imports_stations_channels_and_measurements() {
        let t0 = timestamp("2024-01-01T00:00:00Z");
        let provider = Arc::new(MockProvider {
            stations: vec![station_record("station-1")],
            channels: vec![channel_record("channel-1", "station-1")],
            measurement_pages: Mutex::new(VecDeque::from([MeasurementBatch {
                measurements: vec![
                    measurement_record(1, t0),
                    measurement_record(1, t0),
                    measurement_record(1, t0),
                ],
                last_measurement_datetime: Some(t0),
                batch_size_limit_reached: false,
                timeframe_limit_reached: false,
            }])),
            recorded_queries: Mutex::new(Vec::new()),
            batch_size: 500,
        });
        let station_repo = Arc::new(MockCountingStationRepository {
            stations: Mutex::new(Vec::new()),
        });
        let channel_repo = Arc::new(MockChannelRepository {
            channels: Mutex::new(Vec::new()),
        });
        let measurement_repo = Arc::new(MockMeasurementRepository {
            measurements: Mutex::new(Vec::new()),
        });

        let service = DataImportService::new(
            station_repo.clone(),
            channel_repo.clone(),
            measurement_repo.clone(),
            vec![runtime(provider.clone())],
        );

        let summary = service.import(None, None).expect("import should succeed");

        assert_eq!(summary.data_sources, 1);
        assert_eq!(summary.counting_stations, 1);
        assert_eq!(summary.channels, 1);
        assert_eq!(summary.measurements, 3);

        assert_eq!(station_repo.stations.lock().unwrap().len(), 1);
        assert_eq!(channel_repo.channels.lock().unwrap().len(), 1);
        assert_eq!(measurement_repo.measurements.lock().unwrap().len(), 3);

        // The saved station must be linked to the importing data source.
        let saved_station = &station_repo.stations.lock().unwrap()[0];
        assert_eq!(
            saved_station.data_source_id.map(|id| id.0),
            Some(DataSource::id_from_name("Münster"))
        );
    }

    #[test]
    fn pages_measurements_until_batch_size_limit_is_not_reached() {
        let t1 = timestamp("2024-01-01T10:00:00Z");
        let page_one = MeasurementBatch {
            measurements: (0..500).map(|i| measurement_record(i as i64, t1)).collect(),
            last_measurement_datetime: Some(t1),
            batch_size_limit_reached: true,
            timeframe_limit_reached: false,
        };
        let page_two = MeasurementBatch {
            measurements: vec![measurement_record(1, t1)],
            last_measurement_datetime: Some(t1),
            batch_size_limit_reached: false,
            timeframe_limit_reached: false,
        };
        let provider = Arc::new(MockProvider {
            stations: vec![station_record("station-1")],
            channels: vec![channel_record("channel-1", "station-1")],
            measurement_pages: Mutex::new(VecDeque::from([page_one, page_two])),
            recorded_queries: Mutex::new(Vec::new()),
            batch_size: 500,
        });
        let measurement_repo = Arc::new(MockMeasurementRepository {
            measurements: Mutex::new(Vec::new()),
        });

        let service = DataImportService::new(
            Arc::new(MockCountingStationRepository {
                stations: Mutex::new(Vec::new()),
            }),
            Arc::new(MockChannelRepository {
                channels: Mutex::new(Vec::new()),
            }),
            measurement_repo.clone(),
            vec![runtime(provider.clone())],
        );

        let summary = service.import(None, None).expect("import should succeed");

        assert_eq!(summary.measurements, 501);
        assert_eq!(measurement_repo.measurements.lock().unwrap().len(), 501);

        let queries = provider.recorded_queries.lock().unwrap();
        assert_eq!(queries.len(), 2, "expected exactly two pages");
        assert_eq!(queries[0].from, None, "first page has no start");
        assert_eq!(
            queries[1].from,
            Some(t1),
            "second page resumes from last datetime"
        );
        assert_eq!(
            queries[1]
                .channel
                .external_datasource_id
                .as_ref()
                .map(|e| e.0.as_str()),
            Some("channel-1")
        );
    }

    #[test]
    fn pages_measurements_until_timeframe_limit_is_not_reached() {
        let t1 = timestamp("2024-01-01T10:00:00Z");
        let page_one = MeasurementBatch {
            measurements: vec![measurement_record(1, t1)],
            last_measurement_datetime: Some(t1),
            batch_size_limit_reached: false,
            timeframe_limit_reached: true,
        };
        let page_two = MeasurementBatch {
            measurements: vec![measurement_record(2, t1)],
            last_measurement_datetime: Some(t1),
            batch_size_limit_reached: false,
            timeframe_limit_reached: false,
        };
        let provider = Arc::new(MockProvider {
            stations: vec![station_record("station-1")],
            channels: vec![channel_record("channel-1", "station-1")],
            measurement_pages: Mutex::new(VecDeque::from([page_one, page_two])),
            recorded_queries: Mutex::new(Vec::new()),
            batch_size: 500,
        });
        let measurement_repo = Arc::new(MockMeasurementRepository {
            measurements: Mutex::new(Vec::new()),
        });

        let service = DataImportService::new(
            Arc::new(MockCountingStationRepository {
                stations: Mutex::new(Vec::new()),
            }),
            Arc::new(MockChannelRepository {
                channels: Mutex::new(Vec::new()),
            }),
            measurement_repo.clone(),
            vec![runtime(provider.clone())],
        );

        let summary = service.import(None, None).expect("import should succeed");
        assert_eq!(summary.measurements, 2);
        assert_eq!(measurement_repo.measurements.lock().unwrap().len(), 2);

        let queries = provider.recorded_queries.lock().unwrap();
        assert_eq!(queries.len(), 2, "time-windowed paging must continue");
        assert_eq!(queries[1].from, Some(t1));
    }

    #[test]
    fn does_not_duplicate_already_known_stations_and_channels() {
        let station = station("station-1");
        let channel = channel("channel-1");
        let provider = Arc::new(MockProvider {
            stations: vec![station_record("station-1")],
            channels: vec![channel_record("channel-1", "station-1")],
            measurement_pages: Mutex::new(VecDeque::from([MeasurementBatch {
                measurements: Vec::new(),
                last_measurement_datetime: None,
                batch_size_limit_reached: false,
                timeframe_limit_reached: false,
            }])),
            recorded_queries: Mutex::new(Vec::new()),
            batch_size: 500,
        });
        // The repository already knows both entities by their external id.
        let station_repo = Arc::new(MockCountingStationRepository {
            stations: Mutex::new(vec![station]),
        });
        let channel_repo = Arc::new(MockChannelRepository {
            channels: Mutex::new(vec![channel]),
        });

        let service = DataImportService::new(
            station_repo.clone(),
            channel_repo.clone(),
            Arc::new(MockMeasurementRepository {
                measurements: Mutex::new(Vec::new()),
            }),
            vec![runtime(provider)],
        );

        let summary = service.import(None, None).expect("import should succeed");

        assert_eq!(summary.counting_stations, 0);
        assert_eq!(summary.channels, 0);
        assert_eq!(summary.measurements, 0);
        assert_eq!(station_repo.stations.lock().unwrap().len(), 1);
        assert_eq!(channel_repo.channels.lock().unwrap().len(), 1);
    }

    /// Records every repository save into a shared log so tests can assert the
    /// strict stations -> channels -> measurements ordering.
    struct LoggingRepositories {
        log: Arc<Mutex<Vec<String>>>,
        stations: Mutex<Vec<CountingStation>>,
        channels: Mutex<Vec<Channel>>,
        measurements: Mutex<Vec<Measurement>>,
    }

    impl CountingStationRepository for LoggingRepositories {
        fn save(&self, station: CountingStation) -> Result<(), DomainError> {
            self.log.lock().unwrap().push("station".to_string());
            self.stations.lock().unwrap().push(station);
            Ok(())
        }

        fn find_by_id(&self, id: station_vo::Id) -> Result<CountingStation, DomainError> {
            self.stations
                .lock()
                .unwrap()
                .iter()
                .find(|s| s.id == id)
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
                .find(|s| {
                    s.external_datasource_id.as_ref().map(|e| e.0.as_str())
                        == Some(external_id.0.as_str())
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

    impl ChannelRepository for LoggingRepositories {
        fn save(&self, channel: Channel) -> Result<(), DomainError> {
            self.log.lock().unwrap().push("channel".to_string());
            self.channels.lock().unwrap().push(channel);
            Ok(())
        }

        fn find_by_id(&self, id: channel_vo::Id) -> Result<Channel, DomainError> {
            self.channels
                .lock()
                .unwrap()
                .iter()
                .find(|c| c.id == id)
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
                .filter(|c| c.counting_station_id == station_id)
                .cloned()
                .collect())
        }

        fn find_by_external_datasource_id(
            &self,
            external_id: channel_vo::ExternalDatasourceId,
        ) -> Result<Option<Channel>, DomainError> {
            Ok(self
                .channels
                .lock()
                .unwrap()
                .iter()
                .find(|c| {
                    c.external_datasource_id.as_ref().map(|e| e.0.as_str())
                        == Some(external_id.0.as_str())
                })
                .cloned())
        }

        fn find_filtered(
            &self,
            counting_station_id: Option<channel_vo::CountingStationId>,
            name: Option<&str>,
        ) -> Result<Vec<Channel>, DomainError> {
            let channels = self.channels.lock().unwrap();
            Ok(channels
                .iter()
                .filter(|c| {
                    counting_station_id.is_none_or(|id| c.counting_station_id == id)
                        && name.is_none_or(|n| c.name.0.to_lowercase().contains(&n.to_lowercase()))
                })
                .cloned()
                .collect())
        }
    }

    impl MeasurementRepository for LoggingRepositories {
        fn save(&self, measurement: Measurement) -> Result<(), DomainError> {
            self.log.lock().unwrap().push("measurements".to_string());
            self.measurements.lock().unwrap().push(measurement);
            Ok(())
        }

        fn save_batch(&self, measurements: Vec<Measurement>) -> Result<(), DomainError> {
            self.log.lock().unwrap().push("measurements".to_string());
            self.measurements.lock().unwrap().extend(measurements);
            Ok(())
        }

        fn find_by_id(&self, id: measurement_vo::Id) -> Result<Measurement, DomainError> {
            self.measurements
                .lock()
                .unwrap()
                .iter()
                .find(|m| m.id.0 == id.0)
                .cloned()
                .ok_or(DomainError::NotFound(id.0))
        }

        fn find_all(&self) -> Result<Vec<Measurement>, DomainError> {
            Ok(self.measurements.lock().unwrap().clone())
        }

        fn find_by_channel_id(
            &self,
            channel_id: measurement_vo::ChannelId,
        ) -> Result<Vec<Measurement>, DomainError> {
            Ok(self
                .measurements
                .lock()
                .unwrap()
                .iter()
                .filter(|m| m.channel_id.0 == channel_id.0)
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
                .lock()
                .unwrap()
                .iter()
                .filter(|m| channel_id.is_none_or(|id| m.channel_id.0 == id.0))
                .cloned()
                .collect();
            measurements.sort_by(|a, b| b.timestamp.0.cmp(&a.timestamp.0));
            Ok(measurements.into_iter().skip(offset).take(limit).collect())
        }
    }

    #[test]
    fn update_data_source_updates_stations_then_channels_then_measurements() {
        let t0 = timestamp("2024-01-01T00:00:00Z");
        let provider = Arc::new(MockProvider {
            stations: vec![station_record("station-1")],
            channels: vec![channel_record("channel-1", "station-1")],
            measurement_pages: Mutex::new(VecDeque::from([MeasurementBatch {
                measurements: vec![measurement_record(1, t0), measurement_record(1, t0)],
                last_measurement_datetime: Some(t0),
                batch_size_limit_reached: false,
                timeframe_limit_reached: false,
            }])),
            recorded_queries: Mutex::new(Vec::new()),
            batch_size: 500,
        });
        let log = Arc::new(Mutex::new(Vec::new()));
        let repos = Arc::new(LoggingRepositories {
            log: log.clone(),
            stations: Mutex::new(Vec::new()),
            channels: Mutex::new(Vec::new()),
            measurements: Mutex::new(Vec::new()),
        });
        let station_repo: Arc<dyn CountingStationRepository + Send + Sync> = repos.clone();
        let channel_repo: Arc<dyn ChannelRepository + Send + Sync> = repos.clone();
        let measurement_repo: Arc<dyn MeasurementRepository + Send + Sync> = repos.clone();

        let service =
            DataImportService::new(station_repo, channel_repo, measurement_repo, Vec::new());

        let update = service
            .update_data_source(&runtime(provider.clone()), None, |_| Ok(()))
            .expect("update should succeed");

        assert_eq!(update.processed_measurements, 2);
        assert_eq!(update.last_measurement_timestamp, Some(t0));

        // Strict order per data source: stations first, then channels, then measurements.
        assert_eq!(
            *log.lock().unwrap(),
            vec!["station", "channel", "measurements"]
        );
    }

    #[test]
    fn update_data_source_reports_progress_and_resumes_from_last_updated() {
        let t1 = timestamp("2024-01-01T10:00:00Z");
        let last_updated = timestamp("2024-01-01T09:00:00Z");
        let page_one = MeasurementBatch {
            measurements: vec![measurement_record(1, t1), measurement_record(1, t1)],
            last_measurement_datetime: Some(t1),
            batch_size_limit_reached: true,
            timeframe_limit_reached: false,
        };
        let page_two = MeasurementBatch {
            measurements: vec![measurement_record(1, t1)],
            last_measurement_datetime: Some(t1),
            batch_size_limit_reached: false,
            timeframe_limit_reached: false,
        };
        let provider = Arc::new(MockProvider {
            stations: vec![station_record("station-1")],
            channels: vec![channel_record("channel-1", "station-1")],
            measurement_pages: Mutex::new(VecDeque::from([page_one, page_two])),
            recorded_queries: Mutex::new(Vec::new()),
            batch_size: 500,
        });
        let service = DataImportService::new(
            Arc::new(MockCountingStationRepository {
                stations: Mutex::new(Vec::new()),
            }),
            Arc::new(MockChannelRepository {
                channels: Mutex::new(Vec::new()),
            }),
            Arc::new(MockMeasurementRepository {
                measurements: Mutex::new(Vec::new()),
            }),
            Vec::new(),
        );

        let progress = Arc::new(Mutex::new(Vec::new()));
        let update = service
            .update_data_source(&runtime(provider.clone()), Some(last_updated), |count| {
                progress.lock().unwrap().push(count);
                Ok(())
            })
            .expect("update should succeed");

        assert_eq!(update.processed_measurements, 3);
        assert_eq!(update.last_measurement_timestamp, Some(t1));
        assert_eq!(*progress.lock().unwrap(), vec![2, 3]);

        let queries = provider.recorded_queries.lock().unwrap();
        assert_eq!(queries.len(), 2, "expected exactly two pages");
        assert_eq!(
            queries[0].from,
            Some(last_updated),
            "resumes from the data source's last_updated_at"
        );
        assert_eq!(
            queries[1].from,
            Some(t1),
            "second page resumes from the last batch datetime"
        );
    }

    #[test]
    fn update_data_source_rejects_channel_referencing_unknown_station() {
        let provider = Arc::new(MockProvider {
            stations: vec![station_record("station-1")],
            channels: vec![channel_record("channel-1", "station-99")],
            measurement_pages: Mutex::new(VecDeque::new()),
            recorded_queries: Mutex::new(Vec::new()),
            batch_size: 500,
        });
        let service = DataImportService::new(
            Arc::new(MockCountingStationRepository {
                stations: Mutex::new(Vec::new()),
            }),
            Arc::new(MockChannelRepository {
                channels: Mutex::new(Vec::new()),
            }),
            Arc::new(MockMeasurementRepository {
                measurements: Mutex::new(Vec::new()),
            }),
            Vec::new(),
        );

        let error = service
            .update_data_source(&runtime(provider.clone()), None, |_| Ok(()))
            .expect_err("a channel referencing an unknown station must fail");
        assert!(matches!(error, DomainError::InvalidQuery(_)));
    }

    #[test]
    fn import_applies_the_to_bound_to_measurement_queries() {
        let t1 = timestamp("2024-01-01T10:00:00Z");
        let provider = Arc::new(MockProvider {
            stations: vec![station_record("station-1")],
            channels: vec![channel_record("channel-1", "station-1")],
            measurement_pages: Mutex::new(VecDeque::from([MeasurementBatch {
                measurements: vec![measurement_record(1, t1)],
                last_measurement_datetime: Some(t1),
                batch_size_limit_reached: false,
                timeframe_limit_reached: false,
            }])),
            recorded_queries: Mutex::new(Vec::new()),
            batch_size: 500,
        });
        let service = DataImportService::new(
            Arc::new(MockCountingStationRepository {
                stations: Mutex::new(Vec::new()),
            }),
            Arc::new(MockChannelRepository {
                channels: Mutex::new(Vec::new()),
            }),
            Arc::new(MockMeasurementRepository {
                measurements: Mutex::new(Vec::new()),
            }),
            vec![runtime(provider.clone())],
        );

        let to = timestamp("2024-01-31T00:00:00Z");
        let summary = service
            .import(None, Some(to))
            .expect("import should succeed");
        assert_eq!(summary.measurements, 1);

        let queries = provider.recorded_queries.lock().unwrap();
        assert_eq!(queries.len(), 1);
        assert_eq!(queries[0].to, Some(to), "query must carry the `to` bound");
    }

    #[test]
    fn update_data_source_stops_when_batch_has_no_last_datetime() {
        let t1 = timestamp("2024-01-01T10:00:00Z");
        let provider = Arc::new(MockProvider {
            stations: vec![station_record("station-1")],
            channels: vec![channel_record("channel-1", "station-1")],
            measurement_pages: Mutex::new(VecDeque::from([MeasurementBatch {
                measurements: vec![measurement_record(1, t1)],
                last_measurement_datetime: None,
                batch_size_limit_reached: true,
                timeframe_limit_reached: false,
            }])),
            recorded_queries: Mutex::new(Vec::new()),
            batch_size: 500,
        });
        let service = DataImportService::new(
            Arc::new(MockCountingStationRepository {
                stations: Mutex::new(Vec::new()),
            }),
            Arc::new(MockChannelRepository {
                channels: Mutex::new(Vec::new()),
            }),
            Arc::new(MockMeasurementRepository {
                measurements: Mutex::new(Vec::new()),
            }),
            Vec::new(),
        );

        let update = service
            .update_data_source(&runtime(provider.clone()), None, |_| Ok(()))
            .expect("update should succeed");
        assert_eq!(update.processed_measurements, 1);
        assert_eq!(update.last_measurement_timestamp, None);
        assert_eq!(
            provider.recorded_queries.lock().unwrap().len(),
            1,
            "a missing last datetime must stop paging"
        );
    }
}
