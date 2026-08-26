//! Postgres-backed [`AssetRepository`] implementation: metadata for the binary
//! assets (the content itself lives in S3-compatible object storage).
//!
//! `save` is idempotent on the unique `object_key` (`ON CONFLICT DO UPDATE`).
//! The synchronous `postgres` client must only be used from a blocking context
//! (`spawn_blocking`), matching the other driven repositories.

use crate::core::domain::assets::asset::value_objects::{
    AssetId, ByteSize, ContentType, ObjectKey, Sha256,
};
use crate::core::domain::assets::asset::{Asset, AssetOrigin};
use crate::core::domain::assets::repository_port::AssetRepository;
use crate::core::domain::error::DomainError;

use super::pool::PgPool;

const ASSET_COLUMNS: &str =
    "id, object_key, content_type, byte_size, sha256, origin, created_at, updated_at";

pub struct PostgresAssetRepository {
    pool: PgPool,
}

impl PostgresAssetRepository {
    pub fn new(pool: &PgPool) -> Self {
        Self { pool: pool.clone() }
    }

    fn map_row(row: &postgres::Row) -> Result<Asset, DomainError> {
        let origin: String = row.get("origin");
        let origin = match origin.as_str() {
            "builtin" => AssetOrigin::Builtin,
            "provider" => AssetOrigin::Provider,
            other => {
                return Err(DomainError::Database(format!(
                    "unknown asset origin '{other}'"
                )));
            }
        };
        Ok(Asset {
            id: AssetId(row.get("id")),
            object_key: ObjectKey(row.get("object_key")),
            content_type: ContentType(row.get("content_type")),
            byte_size: ByteSize(row.get("byte_size")),
            sha256: Sha256(row.get("sha256")),
            origin,
            created_at: row.get("created_at"),
            updated_at: row.get("updated_at"),
        })
    }
}

impl AssetRepository for PostgresAssetRepository {
    fn find_by_id(&self, id: AssetId) -> Result<Option<Asset>, DomainError> {
        let mut client = self
            .pool
            .get()
            .map_err(|error| DomainError::Database(error.to_string()))?;
        let row = client
            .query_opt(
                &format!("SELECT {ASSET_COLUMNS} FROM assets WHERE id = $1"),
                &[&id.0],
            )
            .map_err(|error| DomainError::Database(error.to_string()))?;
        row.as_ref().map(Self::map_row).transpose()
    }

    fn find_by_object_key(&self, object_key: &ObjectKey) -> Result<Option<Asset>, DomainError> {
        let mut client = self
            .pool
            .get()
            .map_err(|error| DomainError::Database(error.to_string()))?;
        let row = client
            .query_opt(
                &format!("SELECT {ASSET_COLUMNS} FROM assets WHERE object_key = $1"),
                &[&object_key.0],
            )
            .map_err(|error| DomainError::Database(error.to_string()))?;
        row.as_ref().map(Self::map_row).transpose()
    }

    fn save(&self, asset: Asset) -> Result<Asset, DomainError> {
        let mut client = self
            .pool
            .get()
            .map_err(|error| DomainError::Database(error.to_string()))?;
        let row = client
            .query_one(
                "INSERT INTO assets (id, object_key, content_type, byte_size, sha256, origin, created_at, updated_at)
                 VALUES ($1, $2, $3, $4, $5, $6, now(), now())
                 ON CONFLICT (object_key) DO UPDATE
                   SET content_type = EXCLUDED.content_type,
                       byte_size = EXCLUDED.byte_size,
                       sha256 = EXCLUDED.sha256,
                       origin = EXCLUDED.origin,
                       updated_at = now()
                 RETURNING id, object_key, content_type, byte_size, sha256, origin, created_at, updated_at",
                &[
                    &asset.id.0,
                    &asset.object_key.0,
                    &asset.content_type.0,
                    &asset.byte_size.0,
                    &asset.sha256.0,
                    &asset.origin.as_str(),
                ],
            )
            .map_err(|error| DomainError::Database(error.to_string()))?;
        Self::map_row(&row)
    }

    fn delete(&self, object_key: &ObjectKey) -> Result<(), DomainError> {
        let mut client = self
            .pool
            .get()
            .map_err(|error| DomainError::Database(error.to_string()))?;
        client
            .execute("DELETE FROM assets WHERE object_key = $1", &[&object_key.0])
            .map_err(|error| DomainError::Database(error.to_string()))?;
        Ok(())
    }

    fn list(&self) -> Result<Vec<Asset>, DomainError> {
        let mut client = self
            .pool
            .get()
            .map_err(|error| DomainError::Database(error.to_string()))?;
        let rows = client
            .query(
                &format!("SELECT {ASSET_COLUMNS} FROM assets ORDER BY created_at DESC"),
                &[],
            )
            .map_err(|error| DomainError::Database(error.to_string()))?;
        rows.iter().map(Self::map_row).collect()
    }

    fn all_object_keys(&self) -> Result<Vec<ObjectKey>, DomainError> {
        let mut client = self
            .pool
            .get()
            .map_err(|error| DomainError::Database(error.to_string()))?;
        let rows = client
            .query("SELECT object_key FROM assets", &[])
            .map_err(|error| DomainError::Database(error.to_string()))?;
        Ok(rows
            .iter()
            .map(|row| ObjectKey(row.get::<_, String>(0)))
            .collect())
    }
}

#[cfg(test)]
mod tests {
    use chrono::Utc;
    use testcontainers::runners::SyncRunner;
    use testcontainers_modules::postgres::Postgres;
    use uuid::Uuid;

    use super::*;
    use crate::adapter::driven::postgres::create_pool;
    use crate::core::domain::configuration::configuration::value_objects::DatabaseConfiguration;

    fn asset(object_key: &str) -> Asset {
        Asset {
            id: AssetId(Uuid::new_v4()),
            object_key: ObjectKey(object_key.to_string()),
            content_type: ContentType("image/jpeg".to_string()),
            byte_size: ByteSize(123),
            sha256: Sha256("a".repeat(64)),
            origin: AssetOrigin::Builtin,
            created_at: Utc::now(),
            updated_at: Utc::now(),
        }
    }

    #[test]
    fn save_is_idempotent_on_object_key_and_reads_back() {
        let database_user = "bike_counter_test_user";
        let database_password = "bike_counter_test_password";
        let database_name = "bike_counter_test";
        let container = Postgres::default()
            .with_user(database_user)
            .with_password(database_password)
            .with_db_name(database_name)
            .start()
            .unwrap();
        let url = format!(
            "postgres://127.0.0.1:{}/{}",
            container.get_host_port_ipv4(5432).unwrap(),
            database_name
        );
        let configuration = DatabaseConfiguration::new(
            url,
            database_user.to_string(),
            database_password.to_string(),
            database_name.to_string(),
        )
        .unwrap();
        let pool = create_pool(&configuration).unwrap();
        let repository = PostgresAssetRepository::new(&pool);

        let first = repository
            .save(asset("builtin/station-placeholder.jpg"))
            .unwrap();
        let again = repository
            .save(asset("builtin/station-placeholder.jpg"))
            .unwrap();
        assert_eq!(first.object_key, again.object_key);
        assert_eq!(repository.list().unwrap().len(), 1, "idempotent upsert");

        let found = repository
            .find_by_id(first.id)
            .unwrap()
            .expect("must be found by id");
        assert_eq!(found.content_type.0, "image/jpeg");
        assert_eq!(found.origin, AssetOrigin::Builtin);

        let by_key = repository
            .find_by_object_key(&ObjectKey("builtin/station-placeholder.jpg".to_string()))
            .unwrap()
            .expect("must be found by key");
        assert_eq!(by_key.sha256.0, "a".repeat(64));

        assert_eq!(
            repository.all_object_keys().unwrap(),
            vec![ObjectKey("builtin/station-placeholder.jpg".to_string())]
        );
    }

    #[test]
    fn delete_removes_the_asset_row() {
        let database_user = "bike_counter_test_user";
        let database_password = "bike_counter_test_password";
        let database_name = "bike_counter_test";
        let container = Postgres::default()
            .with_user(database_user)
            .with_password(database_password)
            .with_db_name(database_name)
            .start()
            .unwrap();
        let url = format!(
            "postgres://127.0.0.1:{}/{}",
            container.get_host_port_ipv4(5432).unwrap(),
            database_name
        );
        let configuration = DatabaseConfiguration::new(
            url,
            database_user.to_string(),
            database_password.to_string(),
            database_name.to_string(),
        )
        .unwrap();
        let pool = create_pool(&configuration).unwrap();
        let repository = PostgresAssetRepository::new(&pool);

        let asset = repository.save(asset("builtin/bike-icon.svg")).unwrap();
        assert!(
            repository
                .find_by_object_key(&asset.object_key)
                .unwrap()
                .is_some()
        );

        repository.delete(&asset.object_key).unwrap();
        assert!(
            repository
                .find_by_object_key(&asset.object_key)
                .unwrap()
                .is_none()
        );
    }
}
