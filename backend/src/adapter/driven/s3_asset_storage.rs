//! Garage / S3-compatible driven adapter for the [`AssetStorage`] port.
//!
//! Uses `rust-s3` (the lean S3 client) with `Region::Custom` + path-style
//! addressing, so it works against any S3-compatible server.
//! The **blocking** methods (`ensure_bucket`, `put`, `list_object_keys`,
//! `delete`) run through a dedicated small tokio runtime so they can be called
//! from the import/startup/cleanup `spawn_blocking` contexts; `get_stream` is a
//! plain async method used by the BFF to stream to the browser.
//!
//! `rust-s3` exposes no "create bucket if absent" flag, so `ensure_bucket`
//! issues the S3 `CreateBucket` call directly and treats a `409
//! BucketAlreadyOwnedByYou` response as success (idempotent across restarts).
//! The object-storage key must be allowed to create buckets — the bundled
//! `garage` compose service grants this to its default access key; the
//! Garage image is `FROM scratch`, so no `mc`-style init container can do it.
//! A real failure surfaces via the follow-up `ListObjectsV2` verification.
//!
//! The BFF derives the response headers (Content-Type, ETag, Content-Length)
//! from the asset's DB metadata, so this adapter only moves the bytes.

use std::future::Future;
use std::io;
use std::pin::Pin;
use std::time::Duration;

use futures::{SinkExt, Stream, StreamExt};
use s3::BucketConfiguration;
use s3::bucket::Bucket;
use s3::creds::Credentials;
use s3::region::Region;

use crate::core::domain::assets::asset::value_objects::{ContentType, ObjectKey};
use crate::core::domain::assets::asset_storage_port::{
    AssetObjectInfo, AssetObjectStream, AssetStorage,
};
use crate::core::domain::configuration::configuration::value_objects::AssetStorageConfiguration;
use crate::core::domain::error::DomainError;

/// How many times `ensure_bucket` retries while the object-storage server may
/// still be starting up. The compose healthcheck gates the backend, but the S3
/// API can lag the RPC endpoint slightly, so a short retry absorbs the race.
const ENSURE_BUCKET_ATTEMPTS: u32 = 30;
/// Delay between `ensure_bucket` retries.
const ENSURE_BUCKET_RETRY_DELAY: Duration = Duration::from_secs(1);

pub struct S3AssetStorage {
    bucket: Box<Bucket>,
    /// Bucket name, kept for the associated `Bucket::create_with_path_style` call.
    name: String,
    /// Region (with custom endpoint), reused for bucket creation.
    region: Region,
    /// Credentials, reused for bucket creation.
    credentials: Credentials,
    /// Small runtime used to drive the async rust-s3 calls from blocking
    /// contexts (there is no tokio runtime available inside `spawn_blocking`).
    runtime: tokio::runtime::Runtime,
}

impl S3AssetStorage {
    pub fn new(config: &AssetStorageConfiguration) -> Result<Self, DomainError> {
        let region = Region::Custom {
            region: config.region().to_string(),
            endpoint: config.endpoint().to_string(),
        };
        let credentials = Credentials::new(
            Some(config.access_key()),
            Some(config.secret_key()),
            None,
            None,
            None,
        )
        .map_err(|error| {
            DomainError::Database(format!("invalid object storage credentials: {error}"))
        })?;
        let bucket = Bucket::new(config.bucket(), region.clone(), credentials.clone())
            .map_err(|error| {
                DomainError::Database(format!("invalid object storage bucket: {error}"))
            })?
            .with_path_style();
        let runtime = tokio::runtime::Runtime::new().map_err(|error| {
            DomainError::Database(format!("failed to create storage runtime: {error}"))
        })?;
        Ok(Self {
            bucket,
            name: config.bucket().to_string(),
            region,
            credentials,
            runtime,
        })
    }

    /// Creates the bucket (idempotent) and verifies it is usable by listing it.
    fn create_and_verify_bucket(&self) -> Result<(), DomainError> {
        let bucket = self.bucket.clone();
        let name = self.name.clone();
        let region = self.region.clone();
        let credentials = self.credentials.clone();
        self.runtime.block_on(async move {
            // `CreateBucket` is not idempotent at the protocol level: a second
            // call returns `409 BucketAlreadyOwnedByYou`. Treat 2xx and 409 as
            // success; the list below catches every real failure.
            match Bucket::create_with_path_style(
                &name,
                region,
                credentials,
                BucketConfiguration::default(),
            )
            .await
            {
                Ok(response) if (200..300).contains(&response.response_code) => {
                    tracing::info!("created object storage bucket '{name}'");
                }
                Ok(response) if response.response_code == 409 => {
                    tracing::debug!("object storage bucket '{name}' already exists");
                }
                Ok(response) => {
                    tracing::debug!(
                        "object storage bucket '{name}' create returned HTTP {}",
                        response.response_code
                    );
                }
                Err(error) => {
                    tracing::debug!(
                        "object storage bucket '{name}' create request failed: {error}"
                    );
                }
            }
            bucket
                .list(String::new(), None)
                .await
                .map(|_| ())
                .map_err(|error| {
                    DomainError::Database(format!("failed to list bucket '{name}': {error}"))
                })
        })
    }
}

impl AssetStorage for S3AssetStorage {
    fn ensure_bucket(&self) -> Result<(), DomainError> {
        let mut result = Err(DomainError::Database(
            "object storage bucket could not be created".to_string(),
        ));
        for attempt in 1..=ENSURE_BUCKET_ATTEMPTS {
            match self.create_and_verify_bucket() {
                Ok(()) => return Ok(()),
                Err(error) => {
                    tracing::debug!(
                        "object storage bucket not ready yet (attempt {attempt}/{ENSURE_BUCKET_ATTEMPTS}): {error:?}"
                    );
                    result = Err(error);
                    if attempt < ENSURE_BUCKET_ATTEMPTS {
                        std::thread::sleep(ENSURE_BUCKET_RETRY_DELAY);
                    }
                }
            }
        }
        result.map_err(|error| {
            DomainError::Database(format!(
                "object storage bucket is not usable (is the Garage service healthy?): {error:?}"
            ))
        })
    }

    fn put(
        &self,
        object_key: &ObjectKey,
        content_type: &ContentType,
        bytes: &[u8],
    ) -> Result<AssetObjectInfo, DomainError> {
        let bucket = self.bucket.clone();
        let key = object_key.0.clone();
        let key_for_closure = key.clone();
        let content_type = content_type.0.clone();
        let byte_size = bytes.len() as i64;
        let bytes_owned = bytes.to_vec();
        self.runtime
            .block_on(async move {
                bucket
                    .put_object_with_content_type(&key_for_closure, &bytes_owned, &content_type)
                    .await
                    .map(|_| ())
            })
            .map_err(|error| DomainError::Database(format!("failed to upload '{key}': {error}")))?;
        Ok(AssetObjectInfo { byte_size })
    }

    fn list_object_keys(&self) -> Result<Vec<ObjectKey>, DomainError> {
        let bucket = self.bucket.clone();
        let pages = self
            .runtime
            .block_on(async move { bucket.list(String::new(), None).await })
            .map_err(|error| DomainError::Database(format!("failed to list bucket: {error}")))?;
        Ok(pages
            .into_iter()
            .flat_map(|page| page.contents)
            .map(|entry| ObjectKey(entry.key))
            .collect())
    }

    fn delete(&self, object_key: &ObjectKey) -> Result<(), DomainError> {
        let bucket = self.bucket.clone();
        let key = object_key.0.clone();
        let key_for_closure = key.clone();
        self.runtime
            .block_on(async move { bucket.delete_object(&key_for_closure).await.map(|_| ()) })
            .map_err(|error| DomainError::Database(format!("failed to delete '{key}': {error}")))?;
        Ok(())
    }

    fn get_stream(
        &self,
        object_key: &ObjectKey,
    ) -> Pin<Box<dyn Future<Output = Result<AssetObjectStream, DomainError>> + Send + '_>> {
        let bucket = self.bucket.clone();
        let key = object_key.0.clone();
        Box::pin(async move {
            // Bounded channel: the sender applies backpressure, so a slow
            // browser never buffers the whole object in memory. The futures
            // channel's receiver is itself a `Stream`.
            let (tx, rx) = futures::channel::mpsc::channel::<Result<bytes::Bytes, io::Error>>(8);
            // Consume rust-s3's byte stream (`get_object_stream` returns a
            // `ResponseDataStream`) on a background task, forwarding each chunk
            // into the channel while the caller consumes the receiver (real
            // streaming, no full buffering).
            tokio::spawn(async move {
                let mut stream = match bucket.get_object_stream(&key).await {
                    Ok(stream) => stream,
                    Err(error) => {
                        let mut sender = tx.clone();
                        let _ = sender.send(Err(io::Error::other(error.to_string()))).await;
                        return;
                    }
                };
                while let Some(chunk) = stream.bytes().next().await {
                    let item = chunk.map_err(|error| io::Error::other(error.to_string()));
                    let mut sender = tx.clone();
                    if sender.send(item).await.is_err() {
                        break; // consumer dropped, stop producing
                    }
                }
            });
            let body: Box<dyn Stream<Item = Result<bytes::Bytes, io::Error>> + Send + Unpin> =
                Box::new(rx);
            Ok(AssetObjectStream { body })
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::core::domain::configuration::configuration::value_objects::AssetStorageConfiguration;

    #[test]
    fn new_builds_an_adapter_for_a_configured_endpoint() {
        let config = AssetStorageConfiguration::new(
            "http://garage:3900".to_string(),
            "garageadmin".to_string(),
            "garageadmin-secret".to_string(),
            "bike-counter-images".to_string(),
            "garage".to_string(),
        )
        .unwrap();
        // Construction is lazy (no network I/O), so it must always succeed for
        // valid configuration.
        assert!(S3AssetStorage::new(&config).is_ok());
    }
}
