//! Driven (outbound) port: persistence for per-data-source import runs.

use chrono::{DateTime, Utc};
use uuid::Uuid;

use super::data_source::value_objects::Id as DataSourceId;
use super::import_run::DataImportRun;
use crate::core::domain::error::DomainError;

/// Persistence port for per-data-source import runs. Implemented by the
/// Postgres driven adapter; consumed by the data-source update service (which
/// records each source's run) and by the analytics service (which reads the
/// latest run of a source for the UI).
pub trait DataImportRunRepository: Send + Sync {
    /// Persists a new (RUNNING) run.
    fn insert(&self, run: &DataImportRun) -> Result<(), DomainError>;

    /// Moves a RUNNING run to FINISHED at `finished_at`.
    fn finish(&self, id: Uuid, finished_at: DateTime<Utc>) -> Result<(), DomainError>;

    /// Moves a RUNNING run to FAILED at `finished_at` with a failure message.
    fn fail(&self, id: Uuid, finished_at: DateTime<Utc>, message: &str) -> Result<(), DomainError>;

    /// The newest run of a data source (by `started_at`), if any.
    fn latest_by_data_source(
        &self,
        data_source_id: DataSourceId,
    ) -> Result<Option<DataImportRun>, DomainError>;

    /// Finalizes RUNNING import runs that can no longer be owned by a live
    /// worker: a run whose aggregate `jobs` row is terminal (`FINISHED`/
    /// `FAILED`/`CANCELLED`), or an unlinked run (`job_id IS NULL`) that started
    /// before `older_than`. Returns the number of rows finalized.
    ///
    /// A per-source run is normally transitioned by its owning worker thread,
    /// which may be gone (crash/restart) or stuck in a provider call when its
    /// aggregate job is cancelled, so the run would otherwise stay `RUNNING` and
    /// the UI would show a perpetual "Running". The periodic job watcher calls
    /// this so such orphans are finalized promptly.
    fn finalize_orphaned_running(&self, older_than: DateTime<Utc>) -> Result<u64, DomainError>;
}
