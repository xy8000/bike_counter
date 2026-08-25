use super::measurement::{Measurement, value_objects};
use crate::core::domain::error::DomainError;

pub trait MeasurementRepository {
    fn save(&self, measurement: Measurement) -> Result<(), DomainError>;
    /// Inserts a batch idempotently and returns the number of rows actually
    /// inserted (rows skipped by `ON CONFLICT DO NOTHING` are not counted).
    fn save_batch(&self, measurements: Vec<Measurement>) -> Result<u64, DomainError>;
    fn find_by_id(&self, id: value_objects::Id) -> Result<Measurement, DomainError>;
    fn find_all(&self) -> Result<Vec<Measurement>, DomainError>;
    fn find_by_channel_id(
        &self,
        channel_id: value_objects::ChannelId,
    ) -> Result<Vec<Measurement>, DomainError>;

    /// Returns up to `limit` rows for `offset`-based pagination, newest first,
    /// optionally restricted to one channel. The caller passes `limit + 1` to
    /// detect a following page.
    fn find_page(
        &self,
        channel_id: Option<value_objects::ChannelId>,
        offset: usize,
        limit: usize,
    ) -> Result<Vec<Measurement>, DomainError>;

    /// Sums `value` for every measurement with `from <= timestamp <= to`,
    /// optionally restricted to one channel. `None` means all channels.
    fn sum(
        &self,
        from: chrono::DateTime<chrono::Utc>,
        to: chrono::DateTime<chrono::Utc>,
        channel_id: Option<value_objects::ChannelId>,
    ) -> Result<i64, DomainError>;
}
