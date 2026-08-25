//! Driven (outbound) port for the asset **binary** store (S3-compatible object
//! storage, e.g. MinIO). Named in asset-domain terms ("storage", not "object
//! storage") and split by calling context:
//!
//! - The **blocking** methods (`ensure_bucket`, `put`, `list_object_keys`,
//!   `delete`) are used from the import/startup/cleanup `spawn_blocking`
//!   contexts (the synchronous `postgres` crate cannot run inside a tokio
//!   runtime, so the data-source update job runs on blocking threads).
//! - The **async** `get_stream` (returned as a boxed future so the trait stays
//!   dyn-compatible) is used by the BFF to stream image content to the browser
//!   without buffering the whole file.

use std::future::Future;
use std::pin::Pin;

use futures::Stream;
use tokio_util::bytes::Bytes;

use crate::core::domain::assets::asset::value_objects::{ContentType, ObjectKey};
use crate::core::domain::error::DomainError;

pub trait AssetStorage: Send + Sync {
    /// Creates the configured bucket if it does not exist yet (idempotent).
    fn ensure_bucket(&self) -> Result<(), DomainError>;

    /// Uploads `bytes` under `object_key`. Returns the object info for the DB.
    fn put(
        &self,
        object_key: &ObjectKey,
        content_type: &ContentType,
        bytes: &[u8],
    ) -> Result<AssetObjectInfo, DomainError>;

    /// Every object key currently in the bucket (used by cleanup to detect
    /// orphans, i.e. objects without a row in the `assets` table).
    fn list_object_keys(&self) -> Result<Vec<ObjectKey>, DomainError>;

    /// Removes an object from the bucket (used by the asset cleanup job).
    fn delete(&self, object_key: &ObjectKey) -> Result<(), DomainError>;

    /// Opens a streaming read of an object's content for the BFF.
    fn get_stream(
        &self,
        object_key: &ObjectKey,
    ) -> Pin<Box<dyn Future<Output = Result<AssetObjectStream, DomainError>> + Send + '_>>;
}

/// Result of a `put` — the values the domain persists as asset metadata.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AssetObjectInfo {
    /// ETag reported by the storage (used as the HTTP ETag on streamed content).
    pub etag: String,
    /// Size of the uploaded content in bytes.
    pub byte_size: i64,
}

/// A streaming read of an object's content: a chunk stream that never buffers
/// the whole file. The BFF takes the response headers (Content-Type, ETag,
/// Content-Length) from the asset's DB metadata, so only the body crosses this
/// boundary.
pub struct AssetObjectStream {
    pub body: Box<dyn Stream<Item = Result<Bytes, std::io::Error>> + Send + Unpin>,
}
