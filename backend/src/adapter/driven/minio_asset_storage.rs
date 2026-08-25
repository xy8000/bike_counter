//! MinIO (S3-compatible) driven adapter for the [`AssetStorage`] port.
//!
//! Uses `rust-s3` (the lean S3 client) with `Region::Custom` + path-style
//! addressing. The **blocking** methods (`ensure_bucket`, `put`,
//! `list_object_keys`, `delete`) run through a dedicated small tokio runtime so
//! they can be called from the import/startup/cleanup `spawn_blocking` contexts;
//! `get_stream` is a plain async method used by the BFF to stream to the browser.
//!
//! rust-s3 exposes no bucket-creation call in the API used here, so the bucket
//! itself is provisioned once by a `mc` init container in the docker compose
//! stack; `ensure_bucket` verifies it is reachable (fail-fast at startup).
//!
//! The BFF derives the response headers (Content-Type, ETag, Content-Length)
//! from the asset's DB metadata, so this adapter only moves the bytes.

use std::future::Future;
use std::io;
use std::pin::Pin;
use std::task::{Context, Poll};

use futures::{SinkExt, Stream};
use s3::bucket::Bucket;
use s3::creds::Credentials;
use s3::region::Region;
use tokio::io::AsyncWrite;

use crate::core::domain::assets::asset::value_objects::{ContentType, ObjectKey};
use crate::core::domain::assets::asset_storage_port::{
    AssetObjectInfo, AssetObjectStream, AssetStorage,
};
use crate::core::domain::configuration::configuration::value_objects::AssetStorageConfiguration;
use crate::core::domain::error::DomainError;

pub struct MinioAssetStorage {
    bucket: Bucket,
    /// Small runtime used to drive the async rust-s3 calls from blocking
    /// contexts (there is no tokio runtime available inside `spawn_blocking`).
    runtime: tokio::runtime::Runtime,
}

impl MinioAssetStorage {
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
        .map_err(|error| DomainError::Database(format!("invalid MinIO credentials: {error}")))?;
        let bucket = Bucket::new(config.bucket(), region, credentials)
            .map_err(|error| DomainError::Database(format!("invalid MinIO bucket: {error}")))?
            .with_path_style();
        let runtime = tokio::runtime::Runtime::new().map_err(|error| {
            DomainError::Database(format!("failed to create storage runtime: {error}"))
        })?;
        Ok(Self { bucket, runtime })
    }
}

impl AssetStorage for MinioAssetStorage {
    fn ensure_bucket(&self) -> Result<(), DomainError> {
        // The bucket is provisioned by the docker `mc` init container. Verify it
        // is reachable and that listing works (fail-fast with a clear message).
        let bucket = self.bucket.clone();
        self.runtime
            .block_on(async move { bucket.list(String::new(), None).await.map(|_| ()) })
            .map_err(|error| {
                DomainError::Database(format!(
                    "MinIO bucket is not usable (is the 'mc' init container creating it?): {error}"
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
            // Bounded channel: the ChunkWriter applies backpressure, so a slow
            // browser never buffers the whole object in memory. The futures
            // channel's receiver is itself a `Stream`.
            let (tx, rx) = futures::channel::mpsc::channel::<Result<bytes::Bytes, io::Error>>(8);
            // Stream the object into the channel on a background task while the
            // caller consumes the receiver (real streaming, no full buffering).
            tokio::spawn(async move {
                let mut writer = ChunkWriter { tx: tx.clone() };
                let result = bucket.get_object_stream(&key, &mut writer).await;
                drop(writer);
                if let Err(error) = result {
                    let mut sender = tx.clone();
                    let _ = sender.send(Err(io::Error::other(error.to_string()))).await;
                }
            });
            let body: Box<dyn Stream<Item = Result<bytes::Bytes, io::Error>> + Send + Unpin> =
                Box::new(rx);
            Ok(AssetObjectStream { body })
        })
    }
}

/// An `AsyncWrite` that forwards every chunk into a bounded mpsc channel, used
/// by rust-s3's writer-based `get_object_stream` to produce a real byte stream.
/// The bounded channel applies backpressure: when the consumer is slower than
/// MinIO, `poll_write` returns `Pending` instead of buffering the whole object.
struct ChunkWriter {
    tx: futures::channel::mpsc::Sender<Result<bytes::Bytes, io::Error>>,
}

impl AsyncWrite for ChunkWriter {
    fn poll_write(
        self: Pin<&mut Self>,
        cx: &mut Context<'_>,
        buf: &[u8],
    ) -> Poll<io::Result<usize>> {
        let this = self.get_mut();
        let item = Ok(bytes::Bytes::copy_from_slice(buf));
        // Wait for channel capacity before accepting more bytes.
        match this.tx.poll_ready(cx) {
            Poll::Ready(Ok(())) => {}
            Poll::Ready(Err(error)) => {
                return Poll::Ready(Err(io::Error::other(error.to_string())));
            }
            Poll::Pending => return Poll::Pending,
        }
        if let Err(error) = this.tx.start_send(item) {
            return Poll::Ready(Err(io::Error::other(error.to_string())));
        }
        Poll::Ready(Ok(buf.len()))
    }

    fn poll_flush(self: Pin<&mut Self>, _cx: &mut Context<'_>) -> Poll<io::Result<()>> {
        // The mpsc channel has no buffering beyond the queue itself, so there is
        // nothing to flush.
        Poll::Ready(Ok(()))
    }

    fn poll_shutdown(self: Pin<&mut Self>, _cx: &mut Context<'_>) -> Poll<io::Result<()>> {
        // Dropping the sender (which `get_object_stream` does after writing)
        // closes the stream; no explicit close is needed.
        Poll::Ready(Ok(()))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::core::domain::configuration::configuration::value_objects::AssetStorageConfiguration;

    #[test]
    fn new_builds_an_adapter_for_a_configured_endpoint() {
        let config = AssetStorageConfiguration::new(
            "http://minio:9000".to_string(),
            "minioadmin".to_string(),
            "minioadmin".to_string(),
            "bike-counter-images".to_string(),
            "us-east-1".to_string(),
        )
        .unwrap();
        // Construction is lazy (no network I/O), so it must always succeed for
        // valid configuration.
        assert!(MinioAssetStorage::new(&config).is_ok());
    }
}
