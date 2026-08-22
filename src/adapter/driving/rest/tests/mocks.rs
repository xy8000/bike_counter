//! In-memory repositories that back the router in tests (no database required).

use std::sync::Arc;

use crate::core::domain::channels::channel::Channel;
use crate::core::domain::channels::channel::value_objects as channel_vo;
use crate::core::domain::channels::repository::ChannelRepository;
use crate::core::domain::counting_stations::counting_station::CountingStation;
use crate::core::domain::counting_stations::counting_station::value_objects as station_vo;
use crate::core::domain::counting_stations::repository::CountingStationRepository;
use crate::core::domain::data_source::data_source::DataSource;
use crate::core::domain::data_source::data_source::value_objects as data_source_vo;
use crate::core::domain::data_source::repository::DataSourceRepository;
use crate::core::domain::error::DomainError;
use crate::core::domain::health::{HealthService, HealthStatus, ServiceHealthIndicator};
use crate::core::domain::measurements::measurement::Measurement;
use crate::core::domain::measurements::measurement::value_objects as measurement_vo;
use crate::core::domain::measurements::repository::MeasurementRepository;

pub struct MockCountingStationRepository {
    pub stations: Vec<CountingStation>,
}

impl CountingStationRepository for MockCountingStationRepository {
    fn save(&self, _station: CountingStation) -> Result<(), DomainError> {
        Ok(())
    }

    fn find_by_id(&self, id: station_vo::Id) -> Result<CountingStation, DomainError> {
        self.stations
            .iter()
            .find(|station| station.id.0 == id.0)
            .cloned()
            .ok_or(DomainError::NotFound(id.0))
    }

    fn find_all(&self) -> Result<Vec<CountingStation>, DomainError> {
        Ok(self.stations.clone())
    }

    fn find_by_external_datasource_id(
        &self,
        external_id: station_vo::ExternalDatasourceId,
    ) -> Result<Option<CountingStation>, DomainError> {
        Ok(self
            .stations
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
}

pub struct MockMeasurementRepository {
    pub measurements: Vec<Measurement>,
}

impl MeasurementRepository for MockMeasurementRepository {
    fn save(&self, _measurement: Measurement) -> Result<(), DomainError> {
        Ok(())
    }

    fn save_batch(&self, _measurements: Vec<Measurement>) -> Result<(), DomainError> {
        Ok(())
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
