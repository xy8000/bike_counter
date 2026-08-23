//! Driving (inbound) port for measurement reads. Implemented by
//! `MeasurementService`; consumed by the REST measurements handlers.

use crate::core::domain::error::DomainError;
use crate::core::domain::measurements::measurement::Measurement;
use crate::core::domain::measurements::measurement::value_objects as measurement_vo;

pub trait MeasurementServicePort: Send + Sync {
    /// Lists measurements, optionally filtered by channel, using `offset`/`limit`
    /// pagination (newest first). Returns the page and whether more rows follow.
    fn list(
        &self,
        channel_id: Option<measurement_vo::ChannelId>,
        offset: usize,
        limit: usize,
    ) -> Result<(Vec<Measurement>, bool), DomainError>;

    /// Returns a single measurement; `DomainError::NotFound` if unknown.
    fn find_by_id(&self, id: measurement_vo::Id) -> Result<Measurement, DomainError>;
}
