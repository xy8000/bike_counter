//! Opaque persistent key-value storage for provider state, scoped per data source.

use std::collections::HashMap;

use super::data_source::value_objects::Id;
use crate::core::domain::error::DomainError;

/// Opaque persistent key-value store for provider state, scoped per data source.
///
/// Neither the core nor REST interprets keys or values; only the concrete
/// provider adapter understands its keys.
pub trait PersistentStateStore: Send + Sync {
    /// Returns the full opaque key-value map for a data source (empty if none).
    fn get(&self, data_source_id: Id) -> Result<HashMap<String, String>, DomainError>;

    /// Inserts or updates a single key for a data source (one value per key).
    fn set(&self, data_source_id: Id, key: &str, value: &str) -> Result<(), DomainError>;

    /// Deletes a single key for a data source (idempotent).
    fn delete(&self, data_source_id: Id, key: &str) -> Result<(), DomainError>;

    /// Wipes all state for a data source.
    fn clear(&self, data_source_id: Id) -> Result<(), DomainError>;
}
