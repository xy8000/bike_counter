//! Application service for binary assets (counting-station images): idempotent
//! sync of the built-in images at startup, storage of provider-provided images
//! (content-addressed) and lookups for the BFF.
//!
//! The service is **station-agnostic**: the import service (and not this module)
//! decides which asset a counting station points to.

use std::sync::Arc;

use chrono::Utc;
use sha2::{Digest, Sha256 as Sha2Digest};
use uuid::Uuid;

use crate::core::domain::assets::asset::value_objects::{
    AssetId, ByteSize, ContentType, ObjectKey, Sha256,
};
use crate::core::domain::assets::asset::{Asset, AssetOrigin, BuiltinImage};
use crate::core::domain::assets::asset_storage_port::{AssetObjectInfo, AssetStorage};
use crate::core::domain::assets::repository_port::AssetRepository;
use crate::core::domain::assets::service_port::AssetServicePort;
use crate::core::domain::error::DomainError;

/// Object key of the built-in fallback image every station without a provider
/// image points to.
pub const DEFAULT_IMAGE_OBJECT_KEY: &str = "builtin/station-placeholder.jpg";

pub struct AssetService {
    repository: Arc<dyn AssetRepository>,
    storage: Arc<dyn AssetStorage>,
}

impl AssetService {
    pub fn new(repository: Arc<dyn AssetRepository>, storage: Arc<dyn AssetStorage>) -> Self {
        Self {
            repository,
            storage,
        }
    }

    /// Builds a new [`Asset`] metadata row for content already uploaded.
    fn new_asset(
        object_key: ObjectKey,
        content_type: ContentType,
        byte_size: ByteSize,
        sha256: Sha256,
        origin: AssetOrigin,
    ) -> Asset {
        let now = Utc::now();
        Asset {
            id: AssetId(Uuid::new_v4()),
            object_key,
            content_type,
            byte_size,
            sha256,
            origin,
            created_at: now,
            updated_at: now,
        }
    }

    /// Lowercase hex SHA-256 of `bytes`.
    fn sha256_of(bytes: &[u8]) -> String {
        format!("{:x}", Sha2Digest::digest(bytes))
    }

    /// Uploads `bytes` under `object_key` and persists the resulting metadata.
    fn store(
        &self,
        object_key: ObjectKey,
        content_type: ContentType,
        bytes: &[u8],
        origin: AssetOrigin,
    ) -> Result<Asset, DomainError> {
        let info: AssetObjectInfo = self.storage.put(&object_key, &content_type, bytes)?;
        let asset = Self::new_asset(
            object_key,
            content_type,
            ByteSize(info.byte_size),
            Sha256(Self::sha256_of(bytes)),
            origin,
        );
        self.repository.save(asset)
    }
}

impl AssetServicePort for AssetService {
    fn sync_builtin_images(&self, builtin: &[BuiltinImage]) -> Result<(), DomainError> {
        for image in builtin {
            // Idempotent: skip when a row with the same object key already has
            // the same content hash (re-putting is harmless but unnecessary).
            let up_to_date = self
                .repository
                .find_by_object_key(&image.object_key)?
                .is_some_and(|asset| asset.sha256.0 == Self::sha256_of(&image.bytes));
            if !up_to_date {
                self.store(
                    image.object_key.clone(),
                    image.content_type.clone(),
                    &image.bytes,
                    AssetOrigin::Builtin,
                )?;
            }
        }
        Ok(())
    }

    fn default_asset(&self) -> Result<Asset, DomainError> {
        self.repository
            .find_by_object_key(&ObjectKey(DEFAULT_IMAGE_OBJECT_KEY.to_string()))?
            .ok_or_else(|| {
                DomainError::Database(format!(
                    "default asset '{DEFAULT_IMAGE_OBJECT_KEY}' is not synced"
                ))
            })
    }

    fn store_provider_image(
        &self,
        sha256: Sha256,
        content_type: ContentType,
        bytes: &[u8],
    ) -> Result<Asset, DomainError> {
        // Reject a hash that does not match the actual bytes: the object key is
        // content-addressed, so a mismatch would otherwise persist an object
        // whose key disagrees with its stored sha256.
        let actual = Self::sha256_of(bytes);
        if actual != sha256.0 {
            return Err(DomainError::InvalidQuery(format!(
                "provider image hash {} does not match its content (sha256 {})",
                sha256.0, actual
            )));
        }
        // Content-addressed object key: two stations sharing an image share one
        // object (deduplicated by the sha256).
        let object_key = ObjectKey(format!(
            "provider/{}{}",
            sha256.0,
            extension_for(content_type.0.as_str())
        ));
        // Idempotent: an existing row for this key is returned as-is.
        if let Some(existing) = self.repository.find_by_object_key(&object_key)? {
            return Ok(existing);
        }
        self.store(object_key, content_type, bytes, AssetOrigin::Provider)
    }

    fn find_by_id(&self, id: AssetId) -> Result<Option<Asset>, DomainError> {
        self.repository.find_by_id(id)
    }
}

/// File extension derived from the MIME type for content-addressed object keys.
fn extension_for(content_type: &str) -> &'static str {
    match content_type {
        "image/jpeg" | "image/jpg" => ".jpg",
        "image/png" => ".png",
        "image/webp" => ".webp",
        "image/gif" => ".gif",
        "image/svg+xml" => ".svg",
        "image/avif" => ".avif",
        _ => "",
    }
}

#[cfg(test)]
mod tests {
    use std::collections::HashMap;
    use std::sync::{Arc, Mutex};

    use uuid::Uuid;

    use super::*;
    use crate::core::domain::assets::asset::value_objects::ObjectKey;

    fn builtin(object_key: &str, bytes: &[u8]) -> BuiltinImage {
        BuiltinImage {
            object_key: ObjectKey(object_key.to_string()),
            content_type: ContentType("image/jpeg".to_string()),
            bytes: bytes.to_vec(),
        }
    }

    struct MemoryAssetRepository {
        assets: Mutex<HashMap<String, Asset>>,
    }

    impl MemoryAssetRepository {
        fn new() -> Self {
            Self {
                assets: Mutex::new(HashMap::new()),
            }
        }
    }

    impl AssetRepository for MemoryAssetRepository {
        fn find_by_id(&self, id: AssetId) -> Result<Option<Asset>, DomainError> {
            Ok(self
                .assets
                .lock()
                .unwrap()
                .values()
                .find(|asset| asset.id == id)
                .cloned())
        }

        fn find_by_object_key(&self, object_key: &ObjectKey) -> Result<Option<Asset>, DomainError> {
            Ok(self.assets.lock().unwrap().get(&object_key.0).cloned())
        }

        fn save(&self, asset: Asset) -> Result<Asset, DomainError> {
            self.assets
                .lock()
                .unwrap()
                .insert(asset.object_key.0.clone(), asset.clone());
            Ok(asset)
        }

        fn list(&self) -> Result<Vec<Asset>, DomainError> {
            Ok(self.assets.lock().unwrap().values().cloned().collect())
        }

        fn all_object_keys(&self) -> Result<Vec<ObjectKey>, DomainError> {
            Ok(self
                .assets
                .lock()
                .unwrap()
                .keys()
                .map(|key| ObjectKey(key.clone()))
                .collect())
        }
    }

    struct MemoryAssetStorage {
        objects: Mutex<HashMap<String, Vec<u8>>>,
    }

    impl MemoryAssetStorage {
        fn new() -> Self {
            Self {
                objects: Mutex::new(HashMap::new()),
            }
        }
    }

    impl AssetStorage for MemoryAssetStorage {
        fn ensure_bucket(&self) -> Result<(), DomainError> {
            Ok(())
        }

        fn put(
            &self,
            object_key: &ObjectKey,
            _content_type: &ContentType,
            bytes: &[u8],
        ) -> Result<AssetObjectInfo, DomainError> {
            self.objects
                .lock()
                .unwrap()
                .insert(object_key.0.clone(), bytes.to_vec());
            Ok(AssetObjectInfo {
                byte_size: bytes.len() as i64,
            })
        }

        fn list_object_keys(&self) -> Result<Vec<ObjectKey>, DomainError> {
            Ok(self
                .objects
                .lock()
                .unwrap()
                .keys()
                .map(|key| ObjectKey(key.clone()))
                .collect())
        }

        fn delete(&self, object_key: &ObjectKey) -> Result<(), DomainError> {
            self.objects.lock().unwrap().remove(&object_key.0);
            Ok(())
        }

        fn get_stream(
            &self,
            _object_key: &ObjectKey,
        ) -> std::pin::Pin<
            Box<
                dyn std::future::Future<
                        Output = Result<
                            crate::core::domain::assets::asset_storage_port::AssetObjectStream,
                            DomainError,
                        >,
                    > + Send
                    + '_,
            >,
        > {
            Box::pin(async { unimplemented!("not used in these unit tests") })
        }
    }

    fn service(
        repository: Arc<MemoryAssetRepository>,
        storage: Arc<MemoryAssetStorage>,
    ) -> AssetService {
        AssetService::new(
            repository as Arc<dyn AssetRepository>,
            storage as Arc<dyn AssetStorage>,
        )
    }

    #[test]
    fn sync_builtin_images_uploads_and_persists_each_image() {
        let repository = Arc::new(MemoryAssetRepository::new());
        let storage = Arc::new(MemoryAssetStorage::new());
        let service = service(repository.clone(), storage.clone());

        let bytes = b"placeholder".to_vec();
        service
            .sync_builtin_images(&[builtin(DEFAULT_IMAGE_OBJECT_KEY, &bytes)])
            .unwrap();

        let asset = repository
            .find_by_object_key(&ObjectKey(DEFAULT_IMAGE_OBJECT_KEY.to_string()))
            .unwrap()
            .unwrap();
        assert_eq!(asset.origin, AssetOrigin::Builtin);
        assert_eq!(asset.byte_size.0, bytes.len() as i64);
        assert_eq!(asset.sha256.0, AssetService::sha256_of(&bytes));
        assert_eq!(
            storage.list_object_keys().unwrap(),
            vec![ObjectKey(DEFAULT_IMAGE_OBJECT_KEY.to_string())]
        );
    }

    #[test]
    fn sync_builtin_images_is_idempotent() {
        let repository = Arc::new(MemoryAssetRepository::new());
        let storage = Arc::new(MemoryAssetStorage::new());
        let service = service(repository.clone(), storage.clone());

        let bytes = b"same".to_vec();
        let images = [builtin(DEFAULT_IMAGE_OBJECT_KEY, &bytes)];
        service.sync_builtin_images(&images).unwrap();
        service.sync_builtin_images(&images).unwrap();

        assert_eq!(repository.list().unwrap().len(), 1);
        assert_eq!(storage.list_object_keys().unwrap().len(), 1);
    }

    #[test]
    fn default_asset_resolves_after_sync() {
        let repository = Arc::new(MemoryAssetRepository::new());
        let storage = Arc::new(MemoryAssetStorage::new());
        let service = service(repository.clone(), storage.clone());

        service
            .sync_builtin_images(&[builtin(DEFAULT_IMAGE_OBJECT_KEY, b"x")])
            .unwrap();
        let default = service.default_asset().unwrap();
        assert_eq!(default.object_key.0, DEFAULT_IMAGE_OBJECT_KEY);
    }

    #[test]
    fn default_asset_errors_when_not_synced() {
        let repository = Arc::new(MemoryAssetRepository::new());
        let storage = Arc::new(MemoryAssetStorage::new());
        let service = service(repository, storage);
        assert!(service.default_asset().is_err());
    }

    #[test]
    fn store_provider_image_uses_content_addressed_key_and_deduplicates() {
        let repository = Arc::new(MemoryAssetRepository::new());
        let storage = Arc::new(MemoryAssetStorage::new());
        let service = service(repository.clone(), storage.clone());

        let bytes = b"provider-image".to_vec();
        let sha256 = Sha256(AssetService::sha256_of(&bytes));
        let content_type = ContentType("image/png".to_string());

        let first = service
            .store_provider_image(sha256.clone(), content_type.clone(), &bytes)
            .unwrap();
        let second = service
            .store_provider_image(sha256.clone(), content_type.clone(), &bytes)
            .unwrap();

        assert_eq!(first.id, second.id, "idempotent: same row returned");
        assert_eq!(first.origin, AssetOrigin::Provider);
        assert_eq!(first.object_key.0, format!("provider/{}.png", sha256.0));
        assert_eq!(repository.list().unwrap().len(), 1);
        assert_eq!(storage.list_object_keys().unwrap().len(), 1);
    }

    #[test]
    fn store_provider_image_rejects_a_hash_that_does_not_match_the_bytes() {
        let repository = Arc::new(MemoryAssetRepository::new());
        let storage = Arc::new(MemoryAssetStorage::new());
        let service = service(repository.clone(), storage.clone());

        let bytes = b"actual-content".to_vec();
        let wrong = Sha256("f".repeat(64));
        let result =
            service.store_provider_image(wrong, ContentType("image/png".to_string()), &bytes);

        assert!(matches!(result, Err(DomainError::InvalidQuery(_))));
        assert_eq!(repository.list().unwrap().len(), 0);
        assert_eq!(storage.list_object_keys().unwrap().len(), 0);
    }

    #[test]
    fn extension_for_known_and_unknown_types() {
        assert_eq!(extension_for("image/jpeg"), ".jpg");
        assert_eq!(extension_for("image/png"), ".png");
        assert_eq!(extension_for("application/octet-stream"), "");
        assert_eq!(extension_for("image/webp"), ".webp");
    }

    #[test]
    fn find_by_id_returns_none_for_unknown() {
        let repository = Arc::new(MemoryAssetRepository::new());
        let storage = Arc::new(MemoryAssetStorage::new());
        let service = service(repository, storage);
        assert!(
            service
                .find_by_id(AssetId(Uuid::new_v4()))
                .unwrap()
                .is_none()
        );
    }
}
