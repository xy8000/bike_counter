//! Driving (inbound) port for channel reads. Implemented by
//! `ChannelService`; consumed by the REST channels handlers.

use crate::core::domain::channels::channel::Channel;
use crate::core::domain::channels::channel::value_objects as channel_vo;
use crate::core::domain::error::DomainError;

pub trait ChannelServicePort: Send + Sync {
    /// Lists channels, optionally filtered by counting station and/or a
    /// case-insensitive name substring.
    fn list(
        &self,
        counting_station_id: Option<channel_vo::CountingStationId>,
        name: Option<&str>,
    ) -> Result<Vec<Channel>, DomainError>;

    /// Returns a single channel; `DomainError::NotFound` if unknown.
    fn find_by_id(&self, id: channel_vo::Id) -> Result<Channel, DomainError>;
}
