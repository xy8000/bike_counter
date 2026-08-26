//! Application service exposing measurement reads through the core.

use std::sync::Arc;

use crate::core::domain::error::DomainError;
use crate::core::domain::measurements::measurement::Measurement;
use crate::core::domain::measurements::measurement::value_objects as measurement_vo;
use crate::core::domain::measurements::repository_port::MeasurementRepository;
use crate::core::domain::measurements::service_port::MeasurementServicePort;

pub struct MeasurementService {
    repository: Arc<dyn MeasurementRepository + Send + Sync>,
}

impl MeasurementService {
    pub fn new(repository: Arc<dyn MeasurementRepository + Send + Sync>) -> Self {
        Self { repository }
    }

    /// Lists measurements, optionally filtered by channel, using `offset`/`limit`
    /// pagination (newest first). Returns the page and whether more rows follow.
    pub fn list(
        &self,
        channel_id: Option<measurement_vo::ChannelId>,
        offset: usize,
        limit: usize,
    ) -> Result<(Vec<Measurement>, bool), DomainError> {
        // Fetch one extra row so `has_more` can be computed without a second query.
        let rows = self.repository.find_page(channel_id, offset, limit + 1)?;
        let has_more = rows.len() > limit;
        let measurements = rows.into_iter().take(limit).collect();
        Ok((measurements, has_more))
    }

    /// Returns a single measurement; `DomainError::NotFound` if unknown.
    pub fn find_by_id(&self, id: measurement_vo::Id) -> Result<Measurement, DomainError> {
        self.repository.find_by_id(id)
    }
}

impl MeasurementServicePort for MeasurementService {
    fn list(
        &self,
        channel_id: Option<measurement_vo::ChannelId>,
        offset: usize,
        limit: usize,
    ) -> Result<(Vec<Measurement>, bool), DomainError> {
        self.list(channel_id, offset, limit)
    }

    fn find_by_id(&self, id: measurement_vo::Id) -> Result<Measurement, DomainError> {
        self.find_by_id(id)
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
    use crate::core::domain::measurements::repository_port::MeasurementRepository;

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
            from: chrono::DateTime<chrono::Utc>,
            to: chrono::DateTime<chrono::Utc>,
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
            _from: chrono::DateTime<chrono::Utc>,
            _to: chrono::DateTime<chrono::Utc>,
            _bucket_seconds: i64,
            _origin: chrono::DateTime<chrono::Utc>,
            _timezone: &str,
            _channel_ids: &[measurement_vo::ChannelId],
        ) -> Result<Vec<crate::core::domain::measurements::repository_port::TimeBucket>, DomainError>
        {
            Ok(Vec::new())
        }

        fn sum_buckets_by_channel(
            &self,
            _from: chrono::DateTime<chrono::Utc>,
            _to: chrono::DateTime<chrono::Utc>,
            _bucket_seconds: i64,
            _origin: chrono::DateTime<chrono::Utc>,
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
            _from: chrono::DateTime<chrono::Utc>,
            _to: chrono::DateTime<chrono::Utc>,
            _timezone: &str,
            _channel_ids: &[measurement_vo::ChannelId],
        ) -> Result<
            Vec<crate::core::domain::measurements::repository_port::WeekdayTotal>,
            DomainError,
        > {
            Ok(Vec::new())
        }

        fn sum_hours(
            &self,
            _from: chrono::DateTime<chrono::Utc>,
            _to: chrono::DateTime<chrono::Utc>,
            _timezone: &str,
            _channel_ids: &[measurement_vo::ChannelId],
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
        ) -> Result<
            Vec<crate::core::domain::measurements::repository_port::ChannelHourTotal>,
            DomainError,
        > {
            Ok(Vec::new())
        }
        fn sum_by_channel(
            &self,
            _from: chrono::DateTime<chrono::Utc>,
            _to: chrono::DateTime<chrono::Utc>,
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
        let (measurements, has_more) = service().list(None, 0, 100).unwrap();
        assert_eq!(measurements.len(), 2);
        assert!(!has_more);
    }

    #[test]
    fn list_with_channel_filter_returns_only_matching_measurements() {
        let (measurements, _) = service()
            .list(
                Some(measurement_vo::ChannelId(Uuid::from_u128(0x11))),
                0,
                100,
            )
            .unwrap();
        assert_eq!(measurements.len(), 1);
        assert_eq!(measurements[0].value.0, 1);
    }

    #[test]
    fn list_respects_offset_and_limit_and_reports_has_more() {
        let (first, has_more) = service().list(None, 0, 1).unwrap();
        assert_eq!(first.len(), 1);
        assert!(
            has_more,
            "one extra row was fetched to detect the next page"
        );

        let (second, has_more) = service().list(None, 1, 1).unwrap();
        assert_eq!(second.len(), 1);
        assert!(!has_more);
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
