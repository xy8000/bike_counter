//! The [`Asset`] aggregate and its value objects, plus the plain [`BuiltinImage`]
//! data struct describing images embedded in the binary.
//!
//! An [`Asset`] is a *reference* to binary content stored in object storage:
//! only metadata (object key, content type, size, sha256, origin) is persisted
//! in the database. This module is station-agnostic — a counting station links
//! to an asset via its `image_asset_id`, never the other way round.

use chrono::{DateTime, Utc};

/// Metadata for one binary stored in object storage.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Asset {
    pub id: value_objects::AssetId,
    /// Object key in the storage bucket (globally unique).
    pub object_key: value_objects::ObjectKey,
    /// MIME type of the content (e.g. `image/jpeg`).
    pub content_type: value_objects::ContentType,
    /// Size of the content in bytes.
    pub byte_size: value_objects::ByteSize,
    /// SHA-256 hex digest of the content.
    pub sha256: value_objects::Sha256,
    /// Whether the content ships with the server or came from a provider.
    pub origin: AssetOrigin,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
}

/// Distinguishes server-bundled assets from provider-provided ones, so cleanup
/// and object-key naming can treat them differently.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AssetOrigin {
    Builtin,
    Provider,
}

impl AssetOrigin {
    /// The persisted/lowercase string form (`builtin` | `provider`).
    pub fn as_str(&self) -> &'static str {
        match self {
            AssetOrigin::Builtin => "builtin",
            AssetOrigin::Provider => "provider",
        }
    }
}

/// A built-in image embedded in the backend binary. Deliberately a **plain data
/// struct, not a port**: there is exactly one source (the compiled-in bytes) and
/// nothing to abstract behind a trait.
#[derive(Debug, Clone)]
pub struct BuiltinImage {
    pub object_key: value_objects::ObjectKey,
    pub content_type: value_objects::ContentType,
    pub bytes: Vec<u8>,
}

pub mod value_objects {
    use uuid::Uuid;

    use crate::core::domain::error::DomainError;

    /// Identifier of an asset (UUID, matches the `assets.id` column).
    #[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
    pub struct AssetId(pub Uuid);

    /// Object key of the content inside the storage bucket (globally unique).
    #[derive(Debug, Clone, PartialEq, Eq, Hash)]
    pub struct ObjectKey(pub String);

    /// MIME type of the content.
    #[derive(Debug, Clone, PartialEq, Eq)]
    pub struct ContentType(pub String);

    /// Size of the content in bytes.
    #[derive(Debug, Clone, Copy, PartialEq, Eq)]
    pub struct ByteSize(pub i64);

    /// SHA-256 hex digest of the content (64 lowercase hex chars).
    #[derive(Debug, Clone, PartialEq, Eq)]
    pub struct Sha256(pub String);

    impl ObjectKey {
        pub fn parse(key: impl Into<String>) -> Result<Self, DomainError> {
            let key = key.into();
            if key.trim().is_empty() {
                return Err(DomainError::InvalidQuery(
                    "object_key must not be empty".to_string(),
                ));
            }
            Ok(Self(key))
        }
    }

    impl ContentType {
        pub fn parse(content_type: impl Into<String>) -> Result<Self, DomainError> {
            let content_type = content_type.into();
            if content_type.trim().is_empty() {
                return Err(DomainError::InvalidQuery(
                    "content_type must not be empty".to_string(),
                ));
            }
            Ok(Self(content_type))
        }
    }

    impl Sha256 {
        pub fn parse(sha256: impl Into<String>) -> Result<Self, DomainError> {
            let sha256 = sha256.into();
            if sha256.len() != 64 || !sha256.bytes().all(|byte| byte.is_ascii_hexdigit()) {
                return Err(DomainError::InvalidQuery(
                    "sha256 must be a 64-character lowercase hex digest".to_string(),
                ));
            }
            Ok(Self(sha256))
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::core::domain::error::DomainError;

    #[test]
    fn asset_origin_string_forms_are_stable() {
        assert_eq!(AssetOrigin::Builtin.as_str(), "builtin");
        assert_eq!(AssetOrigin::Provider.as_str(), "provider");
    }

    #[test]
    fn object_key_rejects_empty() {
        assert!(matches!(
            value_objects::ObjectKey::parse(""),
            Err(DomainError::InvalidQuery(_))
        ));
        assert!(matches!(
            value_objects::ObjectKey::parse("   "),
            Err(DomainError::InvalidQuery(_))
        ));
        assert!(value_objects::ObjectKey::parse("builtin/station.jpg").is_ok());
    }

    #[test]
    fn content_type_rejects_empty() {
        assert!(matches!(
            value_objects::ContentType::parse(""),
            Err(DomainError::InvalidQuery(_))
        ));
        assert!(value_objects::ContentType::parse("image/jpeg").is_ok());
    }

    #[test]
    fn sha256_rejects_non_digest() {
        for invalid in ["".to_string(), "abc".to_string(), "xyz".repeat(22)] {
            assert!(
                matches!(
                    value_objects::Sha256::parse(&invalid),
                    Err(DomainError::InvalidQuery(_))
                ),
                "expected reject for {invalid:?}"
            );
        }
    }

    #[test]
    fn sha256_accepts_a_hex_digest() {
        let digest = "a".repeat(64);
        assert_eq!(
            value_objects::Sha256::parse(digest.clone()).unwrap().0,
            digest
        );
    }
}
