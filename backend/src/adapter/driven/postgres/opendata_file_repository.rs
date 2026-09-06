//! Postgres-driven adapter for the opendata file registry
//! ([`OpenDataFileRepository`]), backing the `opendata_files` table. The
//! synchronous `postgres` client must only be used from a blocking context
//! (`spawn_blocking`), matching the other repositories.

use std::str::FromStr;

use uuid::Uuid;

use crate::core::domain::error::DomainError;
use crate::core::domain::opendata::file::{Format, Granularity, OpenDataFile};
use crate::core::domain::opendata::file_repository_port::OpenDataFileRepository;

use super::pool::PgPool;

/// Shared column list for every opendata-file read.
const FILE_COLUMNS: &str =
    "id, object_key, station_id, granularity, period, format, byte_size, sha256, created_at";

pub struct PostgresOpenDataFileRepository {
    pool: PgPool,
}

impl PostgresOpenDataFileRepository {
    pub fn new(pool: &PgPool) -> Self {
        Self { pool: pool.clone() }
    }

    fn map_row(row: &postgres::Row) -> OpenDataFile {
        OpenDataFile {
            id: row.get(0),
            object_key: row.get(1),
            station_id: row.get(2),
            granularity: Granularity::from_str(&row.get::<_, String>(3))
                .expect("the DB CHECK constraint only allows known granularities"),
            period: row.get(4),
            format: Format::from_str(&row.get::<_, String>(5))
                .expect("the DB CHECK constraint only allows known formats"),
            byte_size: row.get(6),
            sha256: row.get(7),
            created_at: row.get(8),
        }
    }
}

impl OpenDataFileRepository for PostgresOpenDataFileRepository {
    fn insert(&self, file: &OpenDataFile) -> Result<(), DomainError> {
        let mut client = self
            .pool
            .get()
            .map_err(|error| DomainError::Database(error.to_string()))?;
        client
            .execute(
                "INSERT INTO opendata_files \
                 (id, object_key, station_id, granularity, period, format, byte_size, sha256, created_at) \
                 VALUES ($1, $2, $3, $4, $5, $6, $7, $8, $9)",
                &[
                    &file.id,
                    &file.object_key,
                    &file.station_id,
                    &file.granularity.as_str(),
                    &file.period,
                    &file.format.as_str(),
                    &file.byte_size,
                    &file.sha256,
                    &file.created_at,
                ],
            )
            .map_err(|error| DomainError::Database(error.to_string()))?;
        Ok(())
    }

    fn find_by_object_key(&self, object_key: &str) -> Result<Option<OpenDataFile>, DomainError> {
        let mut client = self
            .pool
            .get()
            .map_err(|error| DomainError::Database(error.to_string()))?;
        let row = client
            .query_opt(
                &format!("SELECT {FILE_COLUMNS} FROM opendata_files WHERE object_key = $1"),
                &[&object_key],
            )
            .map_err(|error| DomainError::Database(error.to_string()))?;
        Ok(row.as_ref().map(Self::map_row))
    }

    fn list_periods(
        &self,
        granularity: Granularity,
        station_id: Option<Uuid>,
    ) -> Result<Vec<String>, DomainError> {
        let mut client = self
            .pool
            .get()
            .map_err(|error| DomainError::Database(error.to_string()))?;
        let rows = match station_id {
            Some(station_id) => client
                .query(
                    "SELECT DISTINCT period FROM opendata_files \
                     WHERE granularity = $1 AND station_id = $2 ORDER BY period DESC",
                    &[&granularity.as_str(), &station_id],
                )
                .map_err(|error| DomainError::Database(error.to_string()))?,
            None => client
                .query(
                    "SELECT DISTINCT period FROM opendata_files \
                     WHERE granularity = $1 AND station_id IS NULL ORDER BY period DESC",
                    &[&granularity.as_str()],
                )
                .map_err(|error| DomainError::Database(error.to_string()))?,
        };
        Ok(rows.iter().map(|row| row.get(0)).collect())
    }

    fn find_by_period(
        &self,
        granularity: Granularity,
        period: &str,
        station_id: Option<Uuid>,
    ) -> Result<Vec<OpenDataFile>, DomainError> {
        let mut client = self
            .pool
            .get()
            .map_err(|error| DomainError::Database(error.to_string()))?;
        let rows = match station_id {
            Some(station_id) => client
                .query(
                    &format!(
                        "SELECT {FILE_COLUMNS} FROM opendata_files \
                         WHERE granularity = $1 AND period = $2 AND station_id = $3 \
                         ORDER BY format"
                    ),
                    &[&granularity.as_str(), &period, &station_id],
                )
                .map_err(|error| DomainError::Database(error.to_string()))?,
            None => client
                .query(
                    &format!(
                        "SELECT {FILE_COLUMNS} FROM opendata_files \
                         WHERE granularity = $1 AND period = $2 AND station_id IS NULL \
                         ORDER BY format"
                    ),
                    &[&granularity.as_str(), &period],
                )
                .map_err(|error| DomainError::Database(error.to_string()))?,
        };
        Ok(rows.iter().map(Self::map_row).collect())
    }

    fn max_period(
        &self,
        granularity: Granularity,
        station_id: Option<Uuid>,
    ) -> Result<Option<String>, DomainError> {
        let mut client = self
            .pool
            .get()
            .map_err(|error| DomainError::Database(error.to_string()))?;
        let row = match station_id {
            Some(station_id) => client
                .query_one(
                    "SELECT MAX(period) FROM opendata_files \
                     WHERE granularity = $1 AND station_id = $2",
                    &[&granularity.as_str(), &station_id],
                )
                .map_err(|error| DomainError::Database(error.to_string()))?,
            None => client
                .query_one(
                    "SELECT MAX(period) FROM opendata_files \
                     WHERE granularity = $1 AND station_id IS NULL",
                    &[&granularity.as_str()],
                )
                .map_err(|error| DomainError::Database(error.to_string()))?,
        };
        Ok(row.get(0))
    }
}

#[cfg(test)]
mod tests {
    use testcontainers::runners::SyncRunner;
    use testcontainers_modules::postgres::Postgres;
    use uuid::Uuid;

    use super::*;
    use crate::adapter::driven::postgres::create_pool;
    use crate::core::domain::configuration::configuration::value_objects::DatabaseConfiguration;

    #[test]
    fn persists_and_reads_registry_rows() {
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
        let repository = PostgresOpenDataFileRepository::new(&pool);

        let station = Uuid::from_u128(1);
        let global = OpenDataFile {
            id: Uuid::new_v4(),
            object_key: "opendata/measurements/daily/2026/2026-09-05.parquet".to_string(),
            station_id: None,
            granularity: Granularity::Daily,
            period: "2026-09-05".to_string(),
            format: Format::Parquet,
            byte_size: 10,
            sha256: "a".repeat(64),
            created_at: chrono::Utc::now(),
        };
        let station_monthly = OpenDataFile {
            id: Uuid::new_v4(),
            object_key: format!(
                "opendata/stations/{station}/measurements/monthly/2026-09/2026-09.json"
            ),
            station_id: Some(station),
            granularity: Granularity::Monthly,
            period: "2026-09".to_string(),
            format: Format::Json,
            byte_size: 5,
            sha256: "b".repeat(64),
            created_at: chrono::Utc::now(),
        };

        repository.insert(&global).unwrap();
        repository.insert(&station_monthly).unwrap();
        // Duplicate object key is rejected at the DB level.
        assert!(repository.insert(&global).is_err());

        let found = repository
            .find_by_object_key(&global.object_key)
            .unwrap()
            .unwrap();
        assert_eq!(found.period, "2026-09-05");
        assert_eq!(found.format, Format::Parquet);

        assert_eq!(
            repository.list_periods(Granularity::Daily, None).unwrap(),
            vec!["2026-09-05"]
        );
        assert_eq!(
            repository
                .list_periods(Granularity::Monthly, Some(station))
                .unwrap(),
            vec!["2026-09"]
        );
        assert!(
            repository
                .list_periods(Granularity::Daily, Some(station))
                .unwrap()
                .is_empty()
        );

        let daily_files = repository
            .find_by_period(Granularity::Daily, "2026-09-05", None)
            .unwrap();
        assert_eq!(daily_files.len(), 1);
        assert_eq!(
            repository.max_period(Granularity::Daily, None).unwrap(),
            Some("2026-09-05".to_string())
        );
        assert_eq!(
            repository
                .max_period(Granularity::Monthly, Some(station))
                .unwrap(),
            Some("2026-09".to_string())
        );
        assert_eq!(
            repository
                .max_period(Granularity::Daily, Some(station))
                .unwrap(),
            None
        );
    }
}
