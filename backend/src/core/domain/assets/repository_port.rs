//! Driven (outbound) port for the asset **metadata** store (PostgreSQL).
//!
//! Deliberately station-agnostic: it knows nothing about counting stations. The
//! station owns its link to an asset via `CountingStation::image_asset_id`.

use crate::core::domain::assets::asset::Asset;
use crate::core::domain::assets::asset::value_objects;
use crate::core::domain::error::DomainError;

pub trait AssetRepository: Send + Sync {
    fn find_by_id(&self, id: value_objects::AssetId) -> Result<Option<Asset>, DomainError>;

    /// Every asset whose id is in `ids` (missing ids are simply absent).
    ///
    /// The default implementation loops over [`Self::find_by_id`], so in-memory
    /// doubles keep working; the Postgres adapter overrides it with a single
    /// `WHERE id = ANY($1::uuid[])` query, so resolving the images of a station
    /// list never costs one round trip per station.
    fn find_by_ids(&self, ids: &[value_objects::AssetId]) -> Result<Vec<Asset>, DomainError> {
        let mut assets = Vec::with_capacity(ids.len());
        for id in ids {
            if let Some(asset) = self.find_by_id(*id)? {
                assets.push(asset);
            }
        }
        Ok(assets)
    }

    fn find_by_object_key(
        &self,
        object_key: &value_objects::ObjectKey,
    ) -> Result<Option<Asset>, DomainError>;

    /// Persists an asset, idempotent on its unique `object_key` (an existing row
    /// with the same key is updated). Returns the stored asset.
    fn save(&self, asset: Asset) -> Result<Asset, DomainError>;

    /// Removes the asset row for `object_key` (used when a built-in asset is no
    /// longer bundled, so the sync can drop the stale row and its object).
    fn delete(&self, object_key: &value_objects::ObjectKey) -> Result<(), DomainError>;

    /// Every asset row, newest first (used by cleanup for the `assets` side of
    /// the orphan comparison).
    fn list(&self) -> Result<Vec<Asset>, DomainError>;

    /// Every persisted object key (used by cleanup to detect orphans).
    fn all_object_keys(&self) -> Result<Vec<value_objects::ObjectKey>, DomainError>;
}

#[cfg(test)]
mod tests {
    use chrono::Utc;
    use uuid::Uuid;

    use super::*;

    /// A double that only implements the required methods, so
    /// [`AssetRepository::find_by_ids`] is exercised through its default
    /// implementation.
    struct MemoryAssets(Vec<Asset>);

    impl AssetRepository for MemoryAssets {
        fn find_by_id(&self, id: value_objects::AssetId) -> Result<Option<Asset>, DomainError> {
            Ok(self.0.iter().find(|asset| asset.id == id).cloned())
        }

        fn find_by_object_key(
            &self,
            object_key: &value_objects::ObjectKey,
        ) -> Result<Option<Asset>, DomainError> {
            Ok(self
                .0
                .iter()
                .find(|asset| &asset.object_key == object_key)
                .cloned())
        }

        fn save(&self, asset: Asset) -> Result<Asset, DomainError> {
            Ok(asset)
        }

        fn delete(&self, _object_key: &value_objects::ObjectKey) -> Result<(), DomainError> {
            Ok(())
        }

        fn list(&self) -> Result<Vec<Asset>, DomainError> {
            Ok(self.0.clone())
        }

        fn all_object_keys(&self) -> Result<Vec<value_objects::ObjectKey>, DomainError> {
            Ok(Vec::new())
        }
    }

    fn asset(id: u128) -> Asset {
        Asset {
            id: value_objects::AssetId(Uuid::from_u128(id)),
            object_key: value_objects::ObjectKey(format!("builtin/{id}.svg")),
            content_type: value_objects::ContentType("image/svg+xml".to_string()),
            byte_size: value_objects::ByteSize(10),
            sha256: value_objects::Sha256("a".repeat(64)),
            origin: crate::core::domain::assets::asset::AssetOrigin::Builtin,
            created_at: Utc::now(),
            updated_at: Utc::now(),
        }
    }

    #[test]
    fn the_default_batch_lookup_resolves_every_known_id_and_skips_unknowns() {
        let repository = MemoryAssets(vec![asset(1), asset(2)]);

        let found = repository
            .find_by_ids(&[
                value_objects::AssetId(Uuid::from_u128(1)),
                value_objects::AssetId(Uuid::from_u128(99)),
                value_objects::AssetId(Uuid::from_u128(2)),
            ])
            .unwrap();
        assert_eq!(found.len(), 2, "unknown ids are simply absent");
        assert!(repository.find_by_ids(&[]).unwrap().is_empty());
    }
}
