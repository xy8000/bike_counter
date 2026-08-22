//! Imports counting stations, channels and measurements from the configured
//! external data sources. The runtime trigger (CLI / scheduling) is a separate,
//! deferred feature; the capability itself is built and tested here.

use std::sync::Arc;

use chrono::{DateTime, Utc};
use uuid::Uuid;

use crate::core::domain::channels::channel::Channel;
use crate::core::domain::channels::repository::ChannelRepository;
use crate::core::domain::configuration::configuration::value_objects::DataSourceConfiguration;
use crate::core::domain::counting_stations::counting_station::value_objects as station_vo;
use crate::core::domain::counting_stations::repository::CountingStationRepository;
use crate::core::domain::data_source::data_source::value_objects::Id as DataSourceId;
use crate::core::domain::data_source::provider::{DataProvider, MeasurementQuery};
use crate::core::domain::error::DomainError;
use crate::core::domain::measurements::repository::MeasurementRepository;

/// A configured data source together with its built provider.
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
            self.sync_counting_stations(runtime, &mut summary)?;
            let channels = self.sync_channels(runtime, &mut summary)?;
            for channel in &channels {
                self.import_measurements(runtime, channel, from, to, &mut summary)?;
            }
        }

        Ok(summary)
    }

    fn sync_counting_stations(
        &self,
        runtime: &DataSourceRuntime,
        summary: &mut ImportSummary,
    ) -> Result<(), DomainError> {
        let stations = runtime
            .provider
            .get_all_counting_stations()
            .map_err(DomainError::from)?;

        for mut station in stations {
            let already_known = match &station.external_datasource_id {
                Some(external_id) => self
                    .counting_station_repository
                    .find_by_external_datasource_id(external_id.clone())?
                    .is_some(),
                None => false,
            };
            if already_known {
                continue;
            }
            station.id = station_vo::Id(Uuid::new_v4());
            station.data_source_id = Some(station_vo::DataSourceId(runtime.data_source_id.0));
            self.counting_station_repository.save(station)?;
            summary.counting_stations += 1;
        }

        Ok(())
    }

    /// Saves new channels and returns every channel (new + existing) so the
    /// caller can page through its measurements.
    fn sync_channels(
        &self,
        runtime: &DataSourceRuntime,
        summary: &mut ImportSummary,
    ) -> Result<Vec<Channel>, DomainError> {
        let channels = runtime
            .provider
            .get_all_channels()
            .map_err(DomainError::from)?;

        let mut result = Vec::with_capacity(channels.len());
        for channel in channels {
            let existing = match &channel.external_datasource_id {
                Some(external_id) => self
                    .channel_repository
                    .find_by_external_datasource_id(external_id.clone())?,
                None => None,
            };

            match existing {
                Some(existing) => result.push(existing),
                None => {
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
            self.measurement_repository.save_batch(batch.measurements)?;

            // Page while the batch-size limit was reached; use the last
            // measurement datetime as the next start. Guard against a missing
            // last datetime to avoid an endless loop.
            match (
                batch.last_measurement_datetime,
                batch.batch_size_limit_reached,
            ) {
                (Some(last), true) => current_from = Some(last),
                _ => break,
            }
        }

        Ok(())
    }
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
    use crate::core::domain::data_source::provider::{MeasurementBatch, ProviderError};
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

    fn measurement(id: u128, channel_id: Uuid, timestamp: DateTime<Utc>) -> Measurement {
        Measurement {
            id: measurement_vo::Id(Uuid::from_u128(id)),
            channel_id: measurement_vo::ChannelId(channel_id),
            value: measurement_vo::Value(1),
            timestamp: measurement_vo::Timestamp(timestamp),
        }
    }

    fn timestamp(rfc3339: &str) -> DateTime<Utc> {
        DateTime::parse_from_rfc3339(rfc3339)
            .expect("valid timestamp")
            .with_timezone(&Utc)
    }

    /// A provider that serves fixed entities and a queue of measurement pages.
    /// Every measurement query is recorded so tests can verify paging.
    struct MockProvider {
        stations: Vec<CountingStation>,
        channels: Vec<Channel>,
        measurement_pages: Mutex<VecDeque<MeasurementBatch>>,
        recorded_queries: Mutex<Vec<MeasurementQuery>>,
        batch_size: usize,
    }

    impl DataProvider for MockProvider {
        fn check_health(&self) -> HealthStatus {
            HealthStatus::Up
        }

        fn get_all_counting_stations(&self) -> Result<Vec<CountingStation>, ProviderError> {
            Ok(self.stations.clone())
        }

        fn get_all_channels(&self) -> Result<Vec<Channel>, ProviderError> {
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
    }

    #[test]
    fn imports_stations_channels_and_measurements() {
        let station = station("station-1");
        let channel = channel("channel-1");
        let t0 = timestamp("2024-01-01T00:00:00Z");
        let provider = Arc::new(MockProvider {
            stations: vec![station.clone()],
            channels: vec![channel.clone()],
            measurement_pages: Mutex::new(VecDeque::from([MeasurementBatch {
                measurements: vec![
                    measurement(0x100, channel.id.0, t0),
                    measurement(0x101, channel.id.0, t0),
                    measurement(0x102, channel.id.0, t0),
                ],
                last_measurement_datetime: Some(t0),
                batch_size_limit_reached: false,
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
        let channel = channel("channel-1");
        let t1 = timestamp("2024-01-01T10:00:00Z");
        let page_one = MeasurementBatch {
            measurements: (0..500)
                .map(|i| measurement(0x1000 + i, channel.id.0, t1))
                .collect(),
            last_measurement_datetime: Some(t1),
            batch_size_limit_reached: true,
        };
        let page_two = MeasurementBatch {
            measurements: vec![measurement(0x9000, channel.id.0, t1)],
            last_measurement_datetime: Some(t1),
            batch_size_limit_reached: false,
        };
        let provider = Arc::new(MockProvider {
            stations: Vec::new(),
            channels: vec![channel.clone()],
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
        assert_eq!(queries[1].channel.id, channel.id);
    }

    #[test]
    fn does_not_duplicate_already_known_stations_and_channels() {
        let station = station("station-1");
        let channel = channel("channel-1");
        let provider = Arc::new(MockProvider {
            stations: vec![station.clone()],
            channels: vec![channel.clone()],
            measurement_pages: Mutex::new(VecDeque::from([MeasurementBatch {
                measurements: Vec::new(),
                last_measurement_datetime: None,
                batch_size_limit_reached: false,
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
}
