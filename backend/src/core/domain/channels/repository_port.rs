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

    /// Lists channels, optionally filtered by counting station and/or a
    /// case-insensitive name substring.
    fn find_filtered(
        &self,
        counting_station_id: Option<value_objects::CountingStationId>,
        name: Option<&str>,
    ) -> Result<Vec<Channel>, DomainError>;

    /// The ids of every channel of one data source (resolved through its
    /// counting stations). The default is unsupported in in-memory doubles —
    /// channels carry no direct data-source link, so only the Postgres adapter
    /// (which joins `counting_stations`) can answer it.
    fn channel_ids_by_data_source_id(
        &self,
        _data_source_id: crate::core::domain::counting_stations::counting_station::value_objects::DataSourceId,
    ) -> Result<Vec<value_objects::Id>, DomainError> {
        Err(DomainError::Database(
            "channel_ids_by_data_source_id is not supported by this ChannelRepository \
             (only the Postgres adapter resolves channels through their counting station)"
                .to_string(),
        ))
    }
}
