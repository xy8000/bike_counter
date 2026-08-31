use chrono::{DateTime, Utc};

use super::data_source::DataSource;
use super::data_source::value_objects::Id;
use crate::core::domain::error::DomainError;

pub trait DataSourceRepository {
    /// Inserts or updates the data source (keyed by its deterministic id).
    fn upsert(&self, data_source: DataSource) -> Result<(), DomainError>;

    fn find_by_id(&self, id: Id) -> Result<Option<DataSource>, DomainError>;

    fn find_by_name(&self, name: &str) -> Result<Option<DataSource>, DomainError>;

    fn find_all(&self) -> Result<Vec<DataSource>, DomainError>;

    fn delete(&self, id: Id) -> Result<(), DomainError>;

    /// Advances the incremental import watermark to the given timestamp.
    fn update_imported_until(&self, id: Id, timestamp: DateTime<Utc>) -> Result<(), DomainError>;

    /// Clears the incremental import watermark (`imported_until = NULL`), so the
    /// next update re-imports everything for the data source.
    fn clear_imported_until(&self, id: Id) -> Result<(), DomainError>;

    /// Records the wall-clock time the data source was last successfully
    /// updated (per source, so a partial multi-source run still counts the
    /// sources that succeeded).
    fn update_last_updated(&self, id: Id, timestamp: DateTime<Utc>) -> Result<(), DomainError>;
}
