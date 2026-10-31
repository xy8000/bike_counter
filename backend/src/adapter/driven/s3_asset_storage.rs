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
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
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
use crate::core::domain::health::{HealthStatus, ServiceHealthIndicator};

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
    /// Handle used to drive the async rust-s3 calls from blocking contexts
    /// (a blocking thread is not an async context). Always a handle into the
    /// single runtime owned by the composition root; only a standalone test
    /// construction (see [`S3AssetStorage::new`]) keeps its own runtime alive in
    /// `owned_runtime`.
    runtime: tokio::runtime::Handle,
    /// Kept alive for standalone test constructions; `None` in production, where
    /// the runtime is shared with the rest of the process.
    #[cfg(test)]
    owned_runtime: Option<tokio::runtime::Runtime>,
    /// Whether the bucket has been verified at least once. Startup no longer
    /// aborts when the object storage is unreachable, so writes retry the bucket
    /// creation lazily and `/health/providers` reports the current state.
    bucket_ready: AtomicBool,
}

impl S3AssetStorage {
    /// Builds the adapter with its **own** runtime (test-only).
    ///
    /// Production wiring always uses [`S3AssetStorage::with_runtime`], so the two
    /// adapters (images and opendata) share the composition root's runtime
    /// instead of each spinning up a multi-thread one.
    #[cfg(test)]
    pub fn new(config: &AssetStorageConfiguration) -> Result<Self, DomainError> {
        let runtime = tokio::runtime::Runtime::new().map_err(|error| {
            DomainError::Database(format!("failed to create storage runtime: {error}"))
        })?;
        let handle = runtime.handle().clone();
        Self::build(config, handle).map(|storage| storage.with_owned_runtime(runtime))
    }

    /// Builds the adapter on a runtime owned by the composition root.
    pub fn with_runtime(
        config: &AssetStorageConfiguration,
        runtime: tokio::runtime::Handle,
    ) -> Result<Self, DomainError> {
        Self::build(config, runtime)
    }

    #[cfg(test)]
    fn with_owned_runtime(mut self, runtime: tokio::runtime::Runtime) -> Self {
        self.owned_runtime = Some(runtime);
        self
    }

    fn build(
        config: &AssetStorageConfiguration,
        runtime: tokio::runtime::Handle,
    ) -> Result<Self, DomainError> {
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
        Ok(Self {
            bucket,
            name: config.bucket().to_string(),
            region,
            credentials,
            runtime,
            #[cfg(test)]
            owned_runtime: None,
            bucket_ready: AtomicBool::new(false),
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

impl S3AssetStorage {
    /// Verifies the bucket once and records the outcome for the health report.
    fn ensure_bucket_once(&self) -> Result<(), DomainError> {
        let result = self.create_and_verify_bucket();
        self.bucket_ready.store(result.is_ok(), Ordering::SeqCst);
        result
    }

    /// Cheap, bounded probe for `/health/providers`.
    ///
    /// Reuses the cached "ready" state and otherwise attempts the bucket
    /// creation exactly once, so a Garage outage is reported instead of blocking
    /// the endpoint for the full retry window.
    pub fn probe(&self) -> Result<(), String> {
        if self.bucket_ready.load(Ordering::SeqCst) {
            return Ok(());
        }
        self.ensure_bucket_once()
            .map_err(|error| format!("{error:?}"))
    }
}

impl AssetStorage for S3AssetStorage {
    fn ensure_bucket(&self) -> Result<(), DomainError> {
        let mut result = Err(DomainError::Database(
            "object storage bucket could not be created".to_string(),
        ));
        for attempt in 1..=ENSURE_BUCKET_ATTEMPTS {
            match self.ensure_bucket_once() {
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
        // Startup no longer aborts when the object storage is unreachable, so the
        // first write after a Garage restart retries the bucket creation once.
        if !self.bucket_ready.load(Ordering::SeqCst)
            && let Err(error) = self.ensure_bucket_once()
        {
            tracing::debug!("object storage bucket still unavailable before upload: {error:?}");
        }
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

/// Health indicator for one object-storage adapter, reported on
/// `/health/providers`. Informational only: it never gates readiness.
pub struct AssetStorageHealth {
    storage: Arc<S3AssetStorage>,
    name: String,
}

impl AssetStorageHealth {
    pub fn new(name: impl Into<String>, storage: Arc<S3AssetStorage>) -> Self {
        Self {
            storage,
            name: name.into(),
        }
    }
}

impl ServiceHealthIndicator for AssetStorageHealth {
    fn name(&self) -> String {
        self.name.clone()
    }

    fn check(&self) -> HealthStatus {
        match self.storage.probe() {
            Ok(()) => HealthStatus::Up,
            Err(message) => HealthStatus::Down(message),
        }
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

    fn test_configuration() -> AssetStorageConfiguration {
        AssetStorageConfiguration::new(
            "http://garage:3900".to_string(),
            "garageadmin".to_string(),
            "garageadmin-secret".to_string(),
            "bike-counter-images".to_string(),
            "garage".to_string(),
        )
        .unwrap()
    }

    #[test]
    fn with_runtime_does_not_own_a_second_runtime() {
        let runtime = tokio::runtime::Runtime::new().unwrap();
        let storage =
            S3AssetStorage::with_runtime(&test_configuration(), runtime.handle().clone()).unwrap();

        assert!(
            storage.owned_runtime.is_none(),
            "a shared runtime must not be duplicated per adapter"
        );
    }

    #[test]
    fn the_health_indicator_reports_the_cached_bucket_state() {
        let storage = Arc::new(S3AssetStorage::new(&test_configuration()).unwrap());
        let indicator = AssetStorageHealth::new("asset_storage", storage.clone());

        assert_eq!(indicator.name(), "asset_storage");
        // A fresh adapter has not verified the bucket yet; the unit test must not
        // touch the network, so the cached state is asserted directly.
        assert!(!storage.bucket_ready.load(Ordering::SeqCst));
        storage.bucket_ready.store(true, Ordering::SeqCst);
        assert_eq!(indicator.check(), HealthStatus::Up);
    }

    #[test]
    fn probe_reports_the_cached_state_without_touching_the_network() {
        let storage = S3AssetStorage::new(&test_configuration()).unwrap();
        storage.bucket_ready.store(true, Ordering::SeqCst);

        assert!(storage.probe().is_ok());
    }

    #[test]
    fn the_injected_runtime_handle_drives_futures_from_spawn_blocking() {
        let runtime = tokio::runtime::Runtime::new().unwrap();
        let storage =
            S3AssetStorage::with_runtime(&test_configuration(), runtime.handle().clone()).unwrap();

        // This is exactly the bridge every blocking storage method uses: a
        // `spawn_blocking` thread drives an async call through the injected
        // handle instead of nesting a second runtime.
        let value = runtime.block_on(async {
            let handle = storage.runtime.clone();
            tokio::task::spawn_blocking(move || handle.block_on(async { 42u8 }))
                .await
                .expect("the blocking task must finish")
        });

        assert_eq!(value, 42);
    }
}
