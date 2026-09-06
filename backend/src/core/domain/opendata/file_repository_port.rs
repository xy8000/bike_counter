//! Driven (outbound) port for the opendata file registry (PostgreSQL
//! `opendata_files`). The registry is both the append-only ledger of published
//! files and the export job's persisted state: the job computes the missing
//! periods by comparing the available measurement periods with the registered
//! ones and never overwrites an existing row.

use uuid::Uuid;

use super::file::{Granularity, OpenDataFile};
use crate::core::domain::error::DomainError;

pub trait OpenDataFileRepository: Send + Sync {
    /// Persists a new file row. Fails on a duplicate `object_key` or on a
    /// duplicate (scope, granularity, period, format) — the DB unique indexes
    /// back the append-only guarantee.
    fn insert(&self, file: &OpenDataFile) -> Result<(), DomainError>;

    /// Looks a file up by its deterministic object key.
    fn find_by_object_key(&self, object_key: &str) -> Result<Option<OpenDataFile>, DomainError>;

    /// The distinct stored periods of a granularity/scope, newest first
    /// (`station_id: None` = global files). Used by the index endpoints.
    fn list_periods(
        &self,
        granularity: Granularity,
        station_id: Option<Uuid>,
    ) -> Result<Vec<String>, DomainError>;

    /// Every distribution file of one granularity/period/scope, used to render
    /// the year / month index.
    fn find_by_period(
        &self,
        granularity: Granularity,
        period: &str,
        station_id: Option<Uuid>,
    ) -> Result<Vec<OpenDataFile>, DomainError>;

    /// The newest stored period (sortable zero-padded strings, so lexicographic
    /// order equals chronological order) for a granularity/scope, if any.
    fn max_period(
        &self,
        granularity: Granularity,
        station_id: Option<Uuid>,
    ) -> Result<Option<String>, DomainError>;
}
