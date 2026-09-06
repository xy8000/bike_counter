//! Postgres-driven adapter returning export-shaped measurement rows
//! ([`OpenDataMeasurementReader`]). The export rows join measurements →
//! channels → counting stations and render the timestamp in naive local
//! central-European time (`Europe/Berlin`, no offset).

use chrono::{DateTime, NaiveDateTime, Utc};
use uuid::Uuid;

use crate::core::domain::error::DomainError;
use crate::core::domain::opendata::file::Granularity;
use crate::core::domain::opendata::measurement::OpenDataMeasurement;
use crate::core::domain::opendata::measurement_reader_port::OpenDataMeasurementReader;

use super::pool::PgPool;

/// The export timezone, shared with the export job.
const EXPORT_TIMEZONE: &str = "Europe/Berlin";

pub struct PostgresOpenDataMeasurementReader {
    pool: PgPool,
}

impl PostgresOpenDataMeasurementReader {
    pub fn new(pool: &PgPool) -> Self {
        Self { pool: pool.clone() }
    }
}

impl OpenDataMeasurementReader for PostgresOpenDataMeasurementReader {
    fn rows(
        &self,
        from: DateTime<Utc>,
        to: DateTime<Utc>,
        station_id: Option<Uuid>,
    ) -> Result<Vec<OpenDataMeasurement>, DomainError> {
        let mut client = self
            .pool
            .get()
            .map_err(|error| DomainError::Database(error.to_string()))?;
        let rows = client
            .query(
                "SELECT s.id AS station_id, c.id AS channel_id, c.name AS channel_name, \
                        (m.timestamp AT TIME ZONE 'Europe/Berlin')::timestamp AS local_ts, \
                        m.value, m.resolution_seconds \
                 FROM measurements m \
                 JOIN channels c ON c.id = m.channel_id \
                 JOIN counting_stations s ON s.id = c.counting_station_id \
                 WHERE m.timestamp >= $1 AND m.timestamp <= $2 \
                   AND ($3::uuid IS NULL OR s.id = $3) \
                 ORDER BY local_ts ASC, station_id ASC, channel_id ASC",
                &[&from, &to, &station_id],
            )
            .map_err(|error| DomainError::Database(error.to_string()))?;
        Ok(rows
            .iter()
            .map(|row| OpenDataMeasurement {
                station_id: row.get(0),
                channel_id: row.get(1),
                channel_name: row.get(2),
                timestamp: row.get::<_, NaiveDateTime>(3),
                value: row.get(4),
                resolution_seconds: row.get(5),
            })
            .collect())
    }

    fn available_periods(
        &self,
        granularity: Granularity,
        from: DateTime<Utc>,
        to: DateTime<Utc>,
        station_id: Option<Uuid>,
    ) -> Result<Vec<String>, DomainError> {
        let mut client = self
            .pool
            .get()
            .map_err(|error| DomainError::Database(error.to_string()))?;
        let pattern = match granularity {
            Granularity::Daily => "YYYY-MM-DD",
            Granularity::Monthly => "YYYY-MM",
        };
        let rows = client
            .query(
                "SELECT DISTINCT \
                        to_char((m.timestamp AT TIME ZONE 'Europe/Berlin'), $4) AS period \
                 FROM measurements m \
                 JOIN channels c ON c.id = m.channel_id \
                 JOIN counting_stations s ON s.id = c.counting_station_id \
                 WHERE m.timestamp >= $1 AND m.timestamp <= $2 \
                   AND ($3::uuid IS NULL OR s.id = $3) \
                 ORDER BY period ASC",
                &[&from, &to, &station_id, &pattern],
            )
            .map_err(|error| DomainError::Database(error.to_string()))?;
        Ok(rows.iter().map(|row| row.get(0)).collect())
    }
}

#[cfg(test)]
mod tests {
    use std::str::FromStr;

    use testcontainers::ImageExt;
    use testcontainers::runners::SyncRunner;
    use testcontainers_modules::postgres::Postgres;
    use uuid::Uuid;

    use super::*;
    use crate::adapter::driven::postgres::create_pool;
    use crate::core::domain::configuration::configuration::value_objects::DatabaseConfiguration;
    use crate::core::domain::measurements::measurement::Measurement;
    use crate::core::domain::measurements::measurement::value_objects as measurement_vo;

    #[test]
    fn reads_joined_rows_and_available_periods() {
        let database_user = "bike_counter_test_user";
        let database_password = "bike_counter_test_password";
        let database_name = "bike_counter_test";
        let postgres = Postgres::default()
            .with_user(database_user)
            .with_password(database_password)
            .with_db_name(database_name)
            .with_tag("16-alpine")
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

        let station_id = Uuid::from_u128(1);
        let data_source_id = Uuid::from_u128(2);
        let channel_id = Uuid::from_u128(3);
        let mut setup_client = postgres::Config::from_str(configuration.database_url()).unwrap();
        setup_client
            .user(configuration.user())
            .password(configuration.password())
            .dbname(configuration.database_name());
        let mut setup_client = setup_client.connect(postgres::NoTls).unwrap();
        setup_client
            .execute(
                "INSERT INTO data_sources (id, name, provider_type) VALUES ($1, $2, $3)",
                &[&data_source_id, &"Source", &"provider"],
            )
            .unwrap();
        setup_client
            .execute(
                "INSERT INTO counting_stations (id, name, description, data_source_id, timezone) \
                 VALUES ($1, $2, $3, $4, $5)",
                &[
                    &station_id,
                    &"Station",
                    &"desc",
                    &data_source_id,
                    &"Europe/Berlin",
                ],
            )
            .unwrap();
        setup_client
            .execute(
                "INSERT INTO channels (id, counting_station_id, name, description) \
                 VALUES ($1, $2, $3, $4)",
                &[&channel_id, &station_id, &"Channel A", &"desc"],
            )
            .unwrap();

        // Two rows in Berlin on 2026-09-05; one at 23:00Z is already 2026-09-06
        // local (CEST, UTC+2) and must land in a different local day.
        let reader = PostgresOpenDataMeasurementReader::new(&pool);
        let at = |s: &str| DateTime::parse_from_rfc3339(s).unwrap().with_timezone(&Utc);
        for (value, ts) in [
            (42i64, "2026-09-05T10:00:00Z"),
            (7i64, "2026-09-05T23:00:00Z"),
        ] {
            setup_client
                .execute(
                    "INSERT INTO measurements \
                     (id, value, channel_id, timestamp, resolution_seconds) \
                     VALUES ($1, $2, $3, $4, $5)",
                    &[&Uuid::new_v4(), &value, &channel_id, &at(ts), &3600i64],
                )
                .unwrap();
        }

        let from = at("2026-09-05T00:00:00Z");
        let to = at("2026-09-06T23:59:59Z");
        let rows = reader.rows(from, to, None).unwrap();
        assert_eq!(rows.len(), 2);
        assert!(rows.iter().all(|r| r.station_id == station_id));
        assert!(rows.iter().all(|r| r.channel_name == "Channel A"));

        // Daily periods (local): the 23:00Z row is local 2026-09-06 01:00.
        let daily = reader
            .available_periods(Granularity::Daily, from, to, None)
            .unwrap();
        assert_eq!(daily, vec!["2026-09-05", "2026-09-06"]);

        // Station scoping and monthly grouping.
        let scoped = reader
            .available_periods(Granularity::Monthly, from, to, Some(station_id))
            .unwrap();
        assert_eq!(scoped, vec!["2026-09"]);
        assert!(
            reader
                .available_periods(Granularity::Daily, from, to, Some(Uuid::from_u128(99)))
                .unwrap()
                .is_empty()
        );
        let _ = measurement_vo::ResolutionSeconds(3600);
        let _ = Measurement {
            id: measurement_vo::Id(Uuid::new_v4()),
            value: measurement_vo::Value(1),
            channel_id: measurement_vo::ChannelId(channel_id),
            timestamp: measurement_vo::Timestamp(at("2026-09-05T10:00:00Z")),
            resolution_seconds: measurement_vo::ResolutionSeconds(3600),
            interval_end: None,
        };
    }
}
