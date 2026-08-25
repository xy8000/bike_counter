//! Business domain module for binary assets (currently counting-station images).
//!
//! Binary content lives in S3-compatible object storage (e.g. MinIO); PostgreSQL
//! stores only metadata plus the station↔asset link. The domain is deliberately
//! **station-agnostic**: the `CountingStation` aggregate owns the link
//! (`image_asset_id`/`image_sha256`), this module knows nothing about stations.
//!
//! - Model: [`asset::Asset`], [`asset::AssetOrigin`], value objects and the
//!   plain [`asset::BuiltinImage`] data struct.
//! - Driven ports: [`repository_port::AssetRepository`] (metadata/DB) and
//!   [`asset_storage_port::AssetStorage`] (binaries/object storage).
//! - Driving port: [`service_port::AssetServicePort`] (implemented by
//!   `AssetService`).

pub mod asset;
pub mod asset_storage_port;
pub mod repository_port;
pub mod service_port;
