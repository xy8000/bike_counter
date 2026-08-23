use super::measurement::{Measurement, value_objects};
use crate::core::domain::error::DomainError;

pub trait MeasurementRepository {
    fn save(&self, measurement: Measurement) -> Result<(), DomainError>;
    fn save_batch(&self, measurements: Vec<Measurement>) -> Result<(), DomainError>;
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
}
