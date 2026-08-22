use super::channel::{Channel, value_objects};
use crate::core::domain::error::DomainError;

pub trait ChannelRepository {
    fn save(&self, channel: Channel) -> Result<(), DomainError>;
    fn find_by_id(&self, id: value_objects::Id) -> Result<Channel, DomainError>;
    fn find_all(&self) -> Result<Vec<Channel>, DomainError>;
    fn find_by_counting_station_id(
        &self,
        station_id: value_objects::CountingStationId,
    ) -> Result<Vec<Channel>, DomainError>;
    fn find_by_external_datasource_id(
        &self,
        external_id: value_objects::ExternalDatasourceId,
    ) -> Result<Option<Channel>, DomainError>;
}
