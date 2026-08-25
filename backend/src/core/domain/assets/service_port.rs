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
}
