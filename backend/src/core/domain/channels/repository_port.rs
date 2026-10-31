use super::channel::{Channel, value_objects};
use crate::core::domain::error::DomainError;

pub trait ChannelRepository {
    fn save(&self, channel: Channel) -> Result<(), DomainError>;
    /// Refreshes an existing channel's mutable attributes (name, description,
    /// station link).
    fn update(&self, channel: Channel) -> Result<(), DomainError>;
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

    /// The channels of the given counting stations.
    ///
    /// The default implementation filters [`Self::find_all`], so in-memory
    /// doubles keep working unchanged; the Postgres adapter overrides it with a
    /// single `WHERE counting_station_id = ANY($1::uuid[])` query, so a viewport
    /// read never loads the whole channel table.
    fn find_by_station_ids(
        &self,
        station_ids: &[value_objects::CountingStationId],
    ) -> Result<Vec<Channel>, DomainError> {
        if station_ids.is_empty() {
            return Ok(Vec::new());
        }
        Ok(self
            .find_all()?
            .into_iter()
            .filter(|channel| station_ids.contains(&channel.counting_station_id))
            .collect())
    }

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

#[cfg(test)]
mod tests {
    use uuid::Uuid;

    use super::*;

    /// A double that only implements the required methods, so
    /// [`ChannelRepository::find_by_station_ids`] is exercised through its
    /// default implementation.
    struct MemoryChannels(Vec<Channel>);

    impl ChannelRepository for MemoryChannels {
        fn save(&self, _channel: Channel) -> Result<(), DomainError> {
            Ok(())
        }

        fn update(&self, _channel: Channel) -> Result<(), DomainError> {
            Ok(())
        }

        fn find_by_id(&self, id: value_objects::Id) -> Result<Channel, DomainError> {
            self.0
                .iter()
                .find(|channel| channel.id == id)
                .cloned()
                .ok_or(DomainError::NotFound(id.0))
        }

        fn find_all(&self) -> Result<Vec<Channel>, DomainError> {
            Ok(self.0.clone())
        }

        fn find_by_counting_station_id(
            &self,
            station_id: value_objects::CountingStationId,
        ) -> Result<Vec<Channel>, DomainError> {
            Ok(self
                .0
                .iter()
                .filter(|channel| channel.counting_station_id == station_id)
                .cloned()
                .collect())
        }

        fn find_by_external_datasource_id(
            &self,
            _external_id: value_objects::ExternalDatasourceId,
        ) -> Result<Option<Channel>, DomainError> {
            Ok(None)
        }

        fn find_filtered(
            &self,
            _counting_station_id: Option<value_objects::CountingStationId>,
            _name: Option<&str>,
        ) -> Result<Vec<Channel>, DomainError> {
            Ok(self.0.clone())
        }
    }

    fn channel(id: u128, station: u128) -> Channel {
        Channel {
            id: value_objects::Id(Uuid::from_u128(id)),
            counting_station_id: value_objects::CountingStationId(Uuid::from_u128(station)),
            name: value_objects::Name(format!("channel-{id}")),
            description: value_objects::Description("desc".to_string()),
            external_datasource_id: None,
        }
    }

    #[test]
    fn the_default_station_filter_keeps_only_the_requested_stations() {
        let repository = MemoryChannels(vec![channel(1, 10), channel(2, 10), channel(3, 20)]);

        let filtered = repository
            .find_by_station_ids(&[value_objects::CountingStationId(Uuid::from_u128(10))])
            .unwrap();
        assert_eq!(filtered.len(), 2);
        assert!(
            filtered
                .iter()
                .all(|channel| channel.counting_station_id.0 == Uuid::from_u128(10))
        );

        assert!(
            repository.find_by_station_ids(&[]).unwrap().is_empty(),
            "an empty station list must not load anything"
        );
    }

    #[test]
    fn the_default_data_source_lookup_is_unsupported() {
        // Channels carry no direct data-source link, so only the Postgres
        // adapter (which joins counting_stations) can answer this query.
        let repository = MemoryChannels(Vec::new());
        let result = repository.channel_ids_by_data_source_id(
            crate::core::domain::counting_stations::counting_station::value_objects::DataSourceId(
                Uuid::from_u128(1),
            ),
        );
        assert!(matches!(result, Err(DomainError::Database(_))));
    }
}
