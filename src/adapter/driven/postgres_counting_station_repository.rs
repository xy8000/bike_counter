use std::str::FromStr;
use std::sync::Mutex;

use postgres::{Client, Config as PostgresConfig, NoTls};
use refinery::embed_migrations;

use crate::core::domain::configuration::configuration::value_objects::DatabaseConfiguration;
use crate::core::domain::counting_stations::counting_station::{CountingStation, value_objects};
use crate::core::domain::counting_stations::repository::CountingStationRepository;
use crate::core::domain::error::DomainError;

embed_migrations!("migrations");

pub struct PostgresCountingStationRepository {
    client: Mutex<Client>,
}

impl PostgresCountingStationRepository {
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

impl CountingStationRepository for PostgresCountingStationRepository {
    fn save(&self, station: CountingStation) -> Result<(), DomainError> {
        let mut client = self
            .client
            .lock()
            .map_err(|error| DomainError::Database(error.to_string()))?;
        client
            .execute(
                "INSERT INTO counting_stations (id, name, description) VALUES ($1, $2, $3)",
                &[&station.id.0, &station.name.0, &station.description.0],
            )
            .map_err(|error| DomainError::Database(error.to_string()))?;
        Ok(())
    }

    fn find_by_id(&self, id: value_objects::Id) -> Result<CountingStation, DomainError> {
        let mut client = self
            .client
            .lock()
            .map_err(|error| DomainError::Database(error.to_string()))?;
        let row = client
            .query_opt(
                "SELECT id, name, description FROM counting_stations WHERE id = $1",
                &[&id.0],
            )
            .map_err(|error| DomainError::Database(error.to_string()))?
            .ok_or(DomainError::NotFound(id.0))?;
        Ok(CountingStation {
            id: value_objects::Id(row.get(0)),
            name: value_objects::Name(row.get(1)),
            description: value_objects::Description(row.get(2)),
        })
    }

    fn find_all(&self) -> Result<Vec<CountingStation>, DomainError> {
        let mut client = self
            .client
            .lock()
            .map_err(|error| DomainError::Database(error.to_string()))?;
        let rows = client
            .query(
                "SELECT id, name, description FROM counting_stations ORDER BY name ASC",
                &[],
            )
            .map_err(|error| DomainError::Database(error.to_string()))?;
        let mut stations = Vec::with_capacity(rows.len());
        for row in rows {
            stations.push(CountingStation {
                id: value_objects::Id(row.get(0)),
                name: value_objects::Name(row.get(1)),
                description: value_objects::Description(row.get(2)),
            });
        }
        Ok(stations)
    }
}
