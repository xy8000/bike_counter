//! Driven (outbound) port for the asset **metadata** store (PostgreSQL).
//!
//! Deliberately station-agnostic: it knows nothing about counting stations. The
//! station owns its link to an asset via `CountingStation::image_asset_id`.

use crate::core::domain::assets::asset::Asset;
use crate::core::domain::assets::asset::value_objects;
use crate::core::domain::error::DomainError;

pub trait AssetRepository: Send + Sync {
    fn find_by_id(&self, id: value_objects::AssetId) -> Result<Option<Asset>, DomainError>;

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
