use std::str::FromStr;
use std::sync::Mutex;

use postgres::{Client, Config as PostgresConfig, NoTls};
use refinery::embed_migrations;

use crate::core::domain::configuration::configuration::value_objects::DatabaseConfiguration;
use crate::core::domain::error::DomainError;
use crate::core::domain::measurements::measurement::{Measurement, value_objects};
use crate::core::domain::measurements::repository::MeasurementRepository;

embed_migrations!("migrations");

pub struct PostgresMeasurementRepository {
    client: Mutex<Client>,
}

impl PostgresMeasurementRepository {
    pub fn new(configuration: &DatabaseConfiguration) -> Result<Self, DomainError> {
        let mut postgres_config = PostgresConfig::from_str(configuration.database_url())
            .map_err(|error| DomainError::Database(error.to_string()))?;
        postgres_config.user(configuration.user());
        postgres_config.password(configuration.password());
        postgres_config.dbname(configuration.database_name());

        let mut client = postgres_config
            .connect(NoTls)
            .map_err(|error| DomainError::Database(format!("{error:?}")))?;
        migrations::runner()
            .run(&mut client)
            .map_err(|error| DomainError::Database(format!("{error:?}")))?;

        Ok(Self {
            client: Mutex::new(client),
        })
    }
}

impl MeasurementRepository for PostgresMeasurementRepository {
    fn save(&self, measurement: Measurement) -> Result<(), DomainError> {
        let mut client = self
            .client
            .lock()
            .map_err(|error| DomainError::Database(error.to_string()))?;
        client
            .execute(
                "INSERT INTO measurements (id, value, channel_id, timestamp) VALUES ($1, $2, $3, $4)",
                &[&measurement.id.0, &measurement.value.0, &measurement.channel_id.0, &measurement.timestamp.0],
            )
            .map_err(|error| DomainError::Database(error.to_string()))?;
        Ok(())
    }

    fn save_batch(&self, measurements: Vec<Measurement>) -> Result<(), DomainError> {
        let mut client = self
            .client
            .lock()
            .map_err(|error| DomainError::Database(error.to_string()))?;
        let mut transaction = client
            .transaction()
            .map_err(|error| DomainError::Database(error.to_string()))?;

        for measurement in measurements {
            transaction
                .execute(
                    "INSERT INTO measurements (id, value, channel_id, timestamp) VALUES ($1, $2, $3, $4)",
                    &[&measurement.id.0, &measurement.value.0, &measurement.channel_id.0, &measurement.timestamp.0],
                )
                .map_err(|error| DomainError::Database(error.to_string()))?;
        }

        transaction
            .commit()
            .map_err(|error| DomainError::Database(error.to_string()))?;
        Ok(())
    }

    fn find_by_id(&self, id: value_objects::Id) -> Result<Measurement, DomainError> {
        let mut client = self
            .client
            .lock()
            .map_err(|error| DomainError::Database(error.to_string()))?;
        let row = client
            .query_opt(
                "SELECT id, value, channel_id, timestamp FROM measurements WHERE id = $1",
                &[&id.0],
            )
            .map_err(|error| DomainError::Database(error.to_string()))?
            .ok_or(DomainError::NotFound(id.0))?;

        Ok(Measurement {
            id: value_objects::Id(row.get(0)),
            value: value_objects::Value(row.get(1)),
            channel_id: value_objects::ChannelId(row.get(2)),
            timestamp: value_objects::Timestamp(row.get(3)),
        })
    }

    fn find_all(&self) -> Result<Vec<Measurement>, DomainError> {
        let mut client = self
            .client
            .lock()
            .map_err(|error| DomainError::Database(error.to_string()))?;
        let rows = client
            .query(
                "SELECT id, value, channel_id, timestamp FROM measurements ORDER BY timestamp DESC",
                &[],
            )
            .map_err(|error| DomainError::Database(error.to_string()))?;
        let mut measurements = Vec::with_capacity(rows.len());
        for row in rows {
            measurements.push(Measurement {
                id: value_objects::Id(row.get(0)),
                value: value_objects::Value(row.get(1)),
                channel_id: value_objects::ChannelId(row.get(2)),
                timestamp: value_objects::Timestamp(row.get(3)),
            });
        }
        Ok(measurements)
    }

    fn find_by_channel_id(&self, channel_id: value_objects::ChannelId) -> Result<Vec<Measurement>, DomainError> {
        let mut client = self
            .client
            .lock()
            .map_err(|error| DomainError::Database(error.to_string()))?;
        let rows = client
            .query(
                "SELECT id, value, channel_id, timestamp FROM measurements WHERE channel_id = $1 ORDER BY timestamp DESC",
                &[&channel_id.0],
            )
            .map_err(|error| DomainError::Database(error.to_string()))?;
        let mut measurements = Vec::with_capacity(rows.len());
        for row in rows {
            measurements.push(Measurement {
                id: value_objects::Id(row.get(0)),
                value: value_objects::Value(row.get(1)),
                channel_id: value_objects::ChannelId(row.get(2)),
                timestamp: value_objects::Timestamp(row.get(3)),
            });
        }
        Ok(measurements)
    }
}

#[cfg(test)]
mod tests {
    use std::str::FromStr;

    use chrono::{TimeZone, Utc};
    use postgres::{Config as PostgresConfig, NoTls};
    use testcontainers::runners::SyncRunner;
    use testcontainers_modules::postgres::Postgres;
    use uuid::Uuid;

    use super::PostgresMeasurementRepository;
    use crate::core::domain::configuration::configuration::value_objects::DatabaseConfiguration;
    use crate::core::domain::measurements::measurement::{Measurement, value_objects};
    use crate::core::domain::measurements::repository::MeasurementRepository;

    #[test]
    fn persists_and_reads_measurements_in_postgres() {
        let database_user = "bike_counter_test_user";
        let database_password = "bike_counter_test_password";
        let database_name = "bike_counter_test";
        let postgres = Postgres::default()
            .with_user(database_user)
            .with_password(database_password)
            .with_db_name(database_name)
            .start()
            .unwrap();
        let database_url = format!(
            "postgres://127.0.0.1:{}/{}",
            postgres.get_host_port_ipv4(5432).unwrap(),
            database_name
        );
        let configuration = DatabaseConfiguration::new(
            database_url,
            database_user.to_string(),
            database_password.to_string(),
            database_name.to_string(),
        )
        .unwrap();
        let repository = PostgresMeasurementRepository::new(&configuration).unwrap();

        let station_id = Uuid::from_u128(200);
        let setup_channel_id = channel_id().0;
        let mut setup_client = PostgresConfig::from_str(configuration.database_url()).unwrap();
        setup_client
            .user(configuration.user())
            .password(configuration.password())
            .dbname(configuration.database_name());
        let mut setup_client = setup_client.connect(NoTls).unwrap();
        setup_client
            .execute(
                "INSERT INTO counting_stations (id, name, description) VALUES ($1, $2, $3)",
                &[&station_id, &"Test station", &"Test station description"],
            )
            .unwrap();
        setup_client
            .execute(
                "INSERT INTO channels (id, counting_station_id, name, description) VALUES ($1, $2, $3, $4)",
                &[
                    &setup_channel_id,
                    &station_id,
                    &"Test channel",
                    &"Test channel description",
                ],
            )
            .unwrap();

        let first_measurement = measurement(1, 42);
        let measurement_id = first_measurement.id;
        repository.save(first_measurement).unwrap();
        repository.save_batch(vec![measurement(2, 84)]).unwrap();

        let stored = repository.find_by_id(measurement_id).unwrap();

        assert_eq!(stored.id.0, measurement_id.0);
        assert_eq!(stored.value.0, 42);
        assert_eq!(stored.channel_id.0, channel_id().0);
        assert_eq!(stored.timestamp.0, timestamp(1));
    }

    fn measurement(id: u128, value: i64) -> Measurement {
        Measurement {
            id: value_objects::Id(Uuid::from_u128(id)),
            value: value_objects::Value(value),
            channel_id: channel_id(),
            timestamp: value_objects::Timestamp(timestamp(id as i64)),
        }
    }

    fn channel_id() -> value_objects::ChannelId {
        value_objects::ChannelId(Uuid::from_u128(100))
    }

    fn timestamp(seconds: i64) -> chrono::DateTime<Utc> {
        Utc.timestamp_opt(seconds, 0).single().unwrap()
    }
}
