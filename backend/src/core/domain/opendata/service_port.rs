//! Driving (inbound) port for the opendata read model consumed by the REST
//! handlers: the file-registry queries behind the index/file endpoints. The
//! dataset metadata (including the JSON schemata) is static and served by the
//! handler layer.

use uuid::Uuid;

use super::file::{Granularity, OpenDataFile};
use crate::core::domain::error::DomainError;

pub trait OpenDataServicePort: Send + Sync {
    /// The distinct stored periods of a granularity/scope (`station_id: None` =
    /// global files), newest first.
    fn list_periods(
        &self,
        granularity: Granularity,
        station_id: Option<Uuid>,
    ) -> Result<Vec<String>, DomainError>;

    /// Every distribution file of one granularity/period/scope, for the year or
    /// month index payloads.
    fn list_files(
        &self,
        granularity: Granularity,
        period: &str,
        station_id: Option<Uuid>,
    ) -> Result<Vec<OpenDataFile>, DomainError>;

    /// Looks a single file up by its deterministic object key (the file-serving
    /// handler reconstructs the key from the URL path).
    fn find_file(&self, object_key: &str) -> Result<Option<OpenDataFile>, DomainError>;
}
