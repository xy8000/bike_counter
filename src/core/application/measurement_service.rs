//! Application service exposing measurement reads through the core.

use std::sync::Arc;

use crate::core::domain::error::DomainError;
use crate::core::domain::measurements::measurement::Measurement;
use crate::core::domain::measurements::measurement::value_objects as measurement_vo;
use crate::core::domain::measurements::repository::MeasurementRepository;

pub struct MeasurementService {
    repository: Arc<dyn MeasurementRepository + Send + Sync>,
}

impl MeasurementService {
    pub fn new(repository: Arc<dyn MeasurementRepository + Send + Sync>) -> Self {
        Self { repository }
    }

    /// Lists measurements, optionally filtered by channel.
    pub fn list(
        &self,
        channel_id: Option<measurement_vo::ChannelId>,
    ) -> Result<Vec<Measurement>, DomainError> {
        match channel_id {
            Some(channel_id) => self.repository.find_by_channel_id(channel_id),
            None => self.repository.find_all(),
        }
    }

    /// Returns a single measurement; `DomainError::NotFound` if unknown.
    pub fn find_by_id(&self, id: measurement_vo::Id) -> Result<Measurement, DomainError> {
        self.repository.find_by_id(id)
    }
}

#[cfg(test)]
mod tests {
    use std::sync::Arc;

    use chrono::Utc;
    use uuid::Uuid;

    use super::MeasurementService;
    use crate::core::domain::error::DomainError;
    use crate::core::domain::measurements::measurement::Measurement;
    use crate::core::domain::measurements::measurement::value_objects as measurement_vo;
    use crate::core::domain::measurements::repository::MeasurementRepository;

    struct MemoryMeasurementRepository {
        measurements: Vec<Measurement>,
    }

    impl MeasurementRepository for MemoryMeasurementRepository {
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

    fn measurement(id: Uuid, channel_id: Uuid, value: i64) -> Measurement {
        Measurement {
            id: measurement_vo::Id(id),
            value: measurement_vo::Value(value),
            channel_id: measurement_vo::ChannelId(channel_id),
            timestamp: measurement_vo::Timestamp(Utc::now()),
        }
    }

    fn service() -> MeasurementService {
        MeasurementService::new(Arc::new(MemoryMeasurementRepository {
            measurements: vec![
                measurement(Uuid::from_u128(0x21), Uuid::from_u128(0x11), 1),
                measurement(Uuid::from_u128(0x22), Uuid::from_u128(0x12), 2),
            ],
        }))
    }

    #[test]
    fn list_without_filter_returns_all_measurements() {
        let measurements = service().list(None).unwrap();
        assert_eq!(measurements.len(), 2);
    }

    #[test]
    fn list_with_channel_filter_returns_only_matching_measurements() {
        let measurements = service()
            .list(Some(measurement_vo::ChannelId(Uuid::from_u128(0x11))))
            .unwrap();
        assert_eq!(measurements.len(), 1);
        assert_eq!(measurements[0].value.0, 1);
    }

    #[test]
    fn find_by_id_returns_the_measurement() {
        let measurement = service()
            .find_by_id(measurement_vo::Id(Uuid::from_u128(0x21)))
            .unwrap();
        assert_eq!(measurement.value.0, 1);
    }

    #[test]
    fn find_by_unknown_id_is_not_found() {
        assert!(matches!(
            service().find_by_id(measurement_vo::Id(Uuid::from_u128(0x99))),
            Err(DomainError::NotFound(_))
        ));
    }
}
