use std::str::FromStr;
use std::sync::Mutex;

use postgres::{Client, Config as PostgresConfig, NoTls};
use refinery::embed_migrations;

use crate::core::domain::channels::channel::{Channel, value_objects};
use crate::core::domain::channels::repository::ChannelRepository;
use crate::core::domain::configuration::configuration::value_objects::DatabaseConfiguration;
use crate::core::domain::error::DomainError;

embed_migrations!("migrations");

pub struct PostgresChannelRepository {
    client: Mutex<Client>,
}

impl PostgresChannelRepository {
    pub fn new(configuration: &DatabaseConfiguration) -> Result<Self, DomainError> {
        let mut config = PostgresConfig::from_str(configuration.database_url())
            .map_err(|error| DomainError::Database(error.to_string()))?;
        config.user(configuration.user());
        config.password(configuration.password());
        config.dbname(configuration.database_name());
        let mut client = config
            .connect(NoTls)
            .map_err(|error| DomainError::Database(error.to_string()))?;
        migrations::runner()
            .run(&mut client)
            .map_err(|error| DomainError::Database(error.to_string()))?;
        Ok(Self {
            client: Mutex::new(client),
        })
    }
}

impl ChannelRepository for PostgresChannelRepository {
    fn save(&self, channel: Channel) -> Result<(), DomainError> {
        let mut client = self
            .client
            .lock()
            .map_err(|error| DomainError::Database(error.to_string()))?;
        client
            .execute(
                "INSERT INTO channels (id, counting_station_id, name, description) VALUES ($1, $2, $3, $4)",
                &[&channel.id.0, &channel.counting_station_id.0, &channel.name.0, &channel.description.0],
            )
            .map_err(|error| DomainError::Database(error.to_string()))?;
        Ok(())
    }

    fn find_by_id(&self, id: value_objects::Id) -> Result<Channel, DomainError> {
        let mut client = self
            .client
            .lock()
            .map_err(|error| DomainError::Database(error.to_string()))?;
        let row = client
            .query_opt(
                "SELECT id, counting_station_id, name, description FROM channels WHERE id = $1",
                &[&id.0],
            )
            .map_err(|error| DomainError::Database(error.to_string()))?
            .ok_or(DomainError::NotFound(id.0))?;
        Ok(Channel {
            id: value_objects::Id(row.get(0)),
            counting_station_id: value_objects::CountingStationId(row.get(1)),
            name: value_objects::Name(row.get(2)),
            description: value_objects::Description(row.get(3)),
        })
    }
}
