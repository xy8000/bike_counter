//! Driving (inbound) port for the asset application service. Implemented by
//! `AssetService`; consumed by the BFF handlers and the import service.
//!
//! Station-agnostic: it has no notion of counting stations.

use crate::core::domain::assets::asset::value_objects;
use crate::core::domain::assets::asset::{Asset, BuiltinImage};
use crate::core::domain::error::DomainError;

pub trait AssetServicePort: Send + Sync {
    /// Idempotently uploads the built-in images (embedded in the binary) and
    /// registers them as `builtin` assets. Called once at startup.
    fn sync_builtin_images(&self, builtin: &[BuiltinImage]) -> Result<(), DomainError>;

    /// The built-in fallback asset (what stations without a provider image get).
    fn default_asset(&self) -> Result<Asset, DomainError>;

    /// Stores a provider-provided image: uploads the bytes to object storage
    /// (content-addressed key), registers the asset and returns it. Callers use
    /// the returned asset to set `CountingStation::image_asset_id`/`image_sha256`.
    fn store_provider_image(
        &self,
        sha256: value_objects::Sha256,
        content_type: value_objects::ContentType,
        bytes: &[u8],
    ) -> Result<Asset, DomainError>;

    fn find_by_id(&self, id: value_objects::AssetId) -> Result<Option<Asset>, DomainError>;

    /// Resolves several assets at once (missing ids are simply absent).
    ///
    /// The default loops over [`Self::find_by_id`]; [`AssetService`] overrides it
    /// with one repository round trip.
    ///
    /// [`AssetService`]: crate::core::application::asset_service::AssetService
    fn find_by_ids(&self, ids: &[value_objects::AssetId]) -> Result<Vec<Asset>, DomainError> {
        let mut assets = Vec::with_capacity(ids.len());
        for id in ids {
            if let Some(asset) = self.find_by_id(*id)? {
                assets.push(asset);
            }
        }
        Ok(assets)
    }
}

#[cfg(test)]
mod tests {
    use chrono::Utc;
    use uuid::Uuid;

    use super::*;

    /// A double that only implements the required methods, so
    /// [`AssetServicePort::find_by_ids`] is exercised through its default
    /// implementation.
    struct MemoryAssetService(Vec<Asset>);

    impl AssetServicePort for MemoryAssetService {
        fn sync_builtin_images(&self, _builtin: &[BuiltinImage]) -> Result<(), DomainError> {
            Ok(())
        }

        fn default_asset(&self) -> Result<Asset, DomainError> {
            unreachable!("the default find_by_ids never asks for the default asset")
        }

        fn store_provider_image(
            &self,
            _sha256: value_objects::Sha256,
            _content_type: value_objects::ContentType,
            _bytes: &[u8],
        ) -> Result<Asset, DomainError> {
            unreachable!("the default find_by_ids never stores an image")
        }

        fn find_by_id(&self, id: value_objects::AssetId) -> Result<Option<Asset>, DomainError> {
            Ok(self.0.iter().find(|asset| asset.id == id).cloned())
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
    fn the_default_batch_lookup_loops_over_find_by_id() {
        let service = MemoryAssetService(vec![asset(1), asset(2)]);

        let found = service
            .find_by_ids(&[
                value_objects::AssetId(Uuid::from_u128(2)),
                value_objects::AssetId(Uuid::from_u128(42)),
            ])
            .unwrap();
        assert_eq!(found.len(), 1);
        assert_eq!(found[0].id, value_objects::AssetId(Uuid::from_u128(2)));
        assert!(service.find_by_ids(&[]).unwrap().is_empty());
    }
}
