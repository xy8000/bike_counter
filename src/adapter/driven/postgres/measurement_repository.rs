use postgres::types::ToSql;

use crate::core::domain::error::DomainError;
use crate::core::domain::measurements::measurement::{Measurement, value_objects};
use crate::core::domain::measurements::repository::MeasurementRepository;

use super::pool::PgPool;

pub struct PostgresMeasurementRepository {
    pool: PgPool,
}

impl PostgresMeasurementRepository {
    pub fn new(pool: &PgPool) -> Self {
        Self { pool: pool.clone() }
    }
}

impl MeasurementRepository for PostgresMeasurementRepository {
    fn save(&self, measurement: Measurement) -> Result<(), DomainError> {
        let mut client = self
            .pool
            .get()
            .map_err(|error| DomainError::Database(error.to_string()))?;
        client
            .execute(
                "INSERT INTO measurements (id, value, channel_id, timestamp) VALUES ($1, $2, $3, $4) \
                 ON CONFLICT (channel_id, timestamp) DO NOTHING",
                &[&measurement.id.0, &measurement.value.0, &measurement.channel_id.0, &measurement.timestamp.0],
            )
            .map_err(|error| DomainError::Database(error.to_string()))?;
        Ok(())
    }

    fn save_batch(&self, measurements: Vec<Measurement>) -> Result<(), DomainError> {
        if measurements.is_empty() {
            return Ok(());
        }
        let mut client = self
            .pool
            .get()
            .map_err(|error| DomainError::Database(error.to_string()))?;
        let mut transaction = client
            .transaction()
            .map_err(|error| DomainError::Database(error.to_string()))?;

        // A single multi-row INSERT instead of one round trip per measurement.
        // The 65 535 parameter cap allows ~16 383 rows per statement; provider
        // batch sizes are far below this.
        let placeholders: Vec<String> = measurements
            .iter()
            .enumerate()
            .map(|(index, _)| {
                let base = index * 4;
                format!(
                    "(${}, ${}, ${}, ${})",
                    base + 1,
                    base + 2,
                    base + 3,
                    base + 4
                )
            })
            .collect();
        let query = format!(
            "INSERT INTO measurements (id, value, channel_id, timestamp) VALUES {} \
             ON CONFLICT (channel_id, timestamp) DO NOTHING",
            placeholders.join(", ")
        );

        let mut params: Vec<&(dyn ToSql + Sync)> = Vec::with_capacity(measurements.len() * 4);
        for measurement in &measurements {
            params.push(&measurement.id.0);
            params.push(&measurement.value.0);
            params.push(&measurement.channel_id.0);
            params.push(&measurement.timestamp.0);
        }

        transaction
            .execute(&query, &params)
            .map_err(|error| DomainError::Database(error.to_string()))?;

        transaction
            .commit()
            .map_err(|error| DomainError::Database(error.to_string()))?;
        Ok(())
    }

    fn find_by_id(&self, id: value_objects::Id) -> Result<Measurement, DomainError> {
        let mut client = self
            .pool
            .get()
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
            .pool
            .get()
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

    fn find_by_channel_id(
        &self,
        channel_id: value_objects::ChannelId,
    ) -> Result<Vec<Measurement>, DomainError> {
        let mut client = self
            .pool
            .get()
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

    fn find_page(
        &self,
        channel_id: Option<value_objects::ChannelId>,
        offset: usize,
        limit: usize,
    ) -> Result<Vec<Measurement>, DomainError> {
        let mut client = self
            .pool
            .get()
            .map_err(|error| DomainError::Database(error.to_string()))?;
        let limit = limit as i64;
        let offset = offset as i64;
        let rows = match channel_id {
            Some(channel_id) => client
                .query(
                    "SELECT id, value, channel_id, timestamp FROM measurements \
                     WHERE channel_id = $1 ORDER BY timestamp DESC LIMIT $2 OFFSET $3",
                    &[&channel_id.0, &limit, &offset],
                )
                .map_err(|error| DomainError::Database(error.to_string()))?,
            None => client
                .query(
                    "SELECT id, value, channel_id, timestamp FROM measurements \
                     ORDER BY timestamp DESC LIMIT $1 OFFSET $2",
                    &[&limit, &offset],
                )
                .map_err(|error| DomainError::Database(error.to_string()))?,
        };
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
    use crate::adapter::driven::postgres::create_pool;
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
        let pool = create_pool(&configuration).unwrap();
        let repository = PostgresMeasurementRepository::new(&pool);

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

        // Pagination is newest-first with offset/limit.
        let page = repository.find_page(None, 0, 10).unwrap();
        assert_eq!(page.len(), 2);
        assert_eq!(page[0].value.0, 84);
        assert_eq!(page[0].timestamp.0, timestamp(2));
        assert_eq!(page[1].value.0, 42);

        let first_page = repository.find_page(None, 0, 1).unwrap();
        assert_eq!(first_page.len(), 1);
        assert_eq!(first_page[0].value.0, 84);
        let second_page = repository.find_page(None, 1, 1).unwrap();
        assert_eq!(second_page.len(), 1);
        assert_eq!(second_page[0].value.0, 42);

        let channel_page = repository.find_page(Some(channel_id()), 0, 10).unwrap();
        assert_eq!(channel_page.len(), 2);
    }

    #[test]
    fn save_batch_persists_all_rows_in_a_single_statement() {
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
        let pool = create_pool(&configuration).unwrap();
        let repository = PostgresMeasurementRepository::new(&pool);

        let station_id = Uuid::from_u128(300);
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

        let batch = (10..30).map(|i| measurement(i, i as i64)).collect();
        repository.save_batch(batch).unwrap();

        let stored = repository.find_by_channel_id(channel_id()).unwrap();
        assert_eq!(stored.len(), 20);
        // An empty batch is a no-op, not an error.
        repository.save_batch(Vec::new()).unwrap();
        assert_eq!(
            repository.find_by_channel_id(channel_id()).unwrap().len(),
            20
        );
    }

    #[test]
    fn save_batch_is_idempotent_on_the_natural_key() {
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
        let pool = create_pool(&configuration).unwrap();
        let repository = PostgresMeasurementRepository::new(&pool);

        let station_id = Uuid::from_u128(400);
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

        // A partially re-run import must never duplicate (channel_id, timestamp).
        let original = measurement(401, 10);
        repository.save(original.clone()).unwrap();

        let duplicate = Measurement {
            id: value_objects::Id(Uuid::new_v4()),
            ..original.clone()
        };
        repository.save(duplicate.clone()).unwrap();
        repository
            .save_batch(vec![original.clone(), duplicate])
            .unwrap();

        let stored = repository.find_by_channel_id(channel_id()).unwrap();
        assert_eq!(stored.len(), 1, "the natural key must collapse duplicates");
        assert_eq!(stored[0].id.0, original.id.0, "the first write wins");
        assert_eq!(stored[0].value.0, 10);
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
