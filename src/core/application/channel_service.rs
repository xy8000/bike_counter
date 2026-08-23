//! Application service exposing channel reads through the core.

use std::sync::Arc;

use crate::core::domain::channels::channel::Channel;
use crate::core::domain::channels::channel::value_objects as channel_vo;
use crate::core::domain::channels::repository::ChannelRepository;
use crate::core::domain::error::DomainError;

pub struct ChannelService {
    repository: Arc<dyn ChannelRepository + Send + Sync>,
}

impl ChannelService {
    pub fn new(repository: Arc<dyn ChannelRepository + Send + Sync>) -> Self {
        Self { repository }
    }

    /// Lists channels, optionally filtered by counting station.
    pub fn list(
        &self,
        counting_station_id: Option<channel_vo::CountingStationId>,
    ) -> Result<Vec<Channel>, DomainError> {
        match counting_station_id {
            Some(station_id) => self.repository.find_by_counting_station_id(station_id),
            None => self.repository.find_all(),
        }
    }

    /// Returns a single channel; `DomainError::NotFound` if unknown.
    pub fn find_by_id(&self, id: channel_vo::Id) -> Result<Channel, DomainError> {
        self.repository.find_by_id(id)
    }
}

#[cfg(test)]
mod tests {
    use std::sync::Arc;

    use uuid::Uuid;

    use super::ChannelService;
    use crate::core::domain::channels::channel::Channel;
    use crate::core::domain::channels::channel::value_objects as channel_vo;
    use crate::core::domain::channels::repository::ChannelRepository;
    use crate::core::domain::error::DomainError;

    struct MemoryChannelRepository {
        channels: Vec<Channel>,
    }

    impl ChannelRepository for MemoryChannelRepository {
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
            _external_id: channel_vo::ExternalDatasourceId,
        ) -> Result<Option<Channel>, DomainError> {
            Ok(None)
        }
    }

    fn channel(id: Uuid, station_id: Uuid, name: &str) -> Channel {
        Channel {
            id: channel_vo::Id(id),
            counting_station_id: channel_vo::CountingStationId(station_id),
            name: channel_vo::Name(name.to_string()),
            description: channel_vo::Description(String::new()),
            external_datasource_id: None,
        }
    }

    fn service() -> ChannelService {
        ChannelService::new(Arc::new(MemoryChannelRepository {
            channels: vec![
                channel(Uuid::from_u128(0x11), Uuid::from_u128(0x1), "A1"),
                channel(Uuid::from_u128(0x12), Uuid::from_u128(0x2), "B1"),
            ],
        }))
    }

    #[test]
    fn list_without_filter_returns_all_channels() {
        let channels = service().list(None).unwrap();
        assert_eq!(channels.len(), 2);
    }

    #[test]
    fn list_with_station_filter_returns_only_matching_channels() {
        let channels = service()
            .list(Some(channel_vo::CountingStationId(Uuid::from_u128(0x1))))
            .unwrap();
        assert_eq!(channels.len(), 1);
        assert_eq!(channels[0].name.0, "A1");
    }

    #[test]
    fn find_by_id_returns_the_channel() {
        let channel = service()
            .find_by_id(channel_vo::Id(Uuid::from_u128(0x11)))
            .unwrap();
        assert_eq!(channel.name.0, "A1");
    }

    #[test]
    fn find_by_unknown_id_is_not_found() {
        assert!(matches!(
            service().find_by_id(channel_vo::Id(Uuid::from_u128(0x99))),
            Err(DomainError::NotFound(_))
        ));
    }
}
