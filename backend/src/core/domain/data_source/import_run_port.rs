//! Driven (outbound) port: persistence for per-data-source import runs.

use chrono::{DateTime, Utc};
use uuid::Uuid;

use super::data_source::value_objects::Id as DataSourceId;
use super::import_run::{DataImportRun, ImportRunFacts, ImportRunPage, ImportRunStatus};
use crate::core::domain::error::DomainError;

/// Persistence port for per-data-source import runs. Implemented by the
/// Postgres driven adapter; consumed by the data-source update service (which
/// records each source's run) and by the analytics service (which reads the
/// latest run of a source for the UI).
pub trait DataImportRunRepository: Send + Sync {
    /// Persists a new (RUNNING) run.
    fn insert(&self, run: &DataImportRun) -> Result<(), DomainError>;

    /// Updates the in-flight outcome counters of a RUNNING run. A run that is
    /// already terminal is left untouched, so late progress reports can never
    /// overwrite the final snapshot. The default is a no-op so in-memory test
    /// doubles keep compiling.
    fn update_progress(&self, _id: Uuid, _facts: ImportRunFacts) -> Result<(), DomainError> {
        Ok(())
    }

    /// Moves a RUNNING run to FINISHED at `finished_at`.
    fn finish(&self, id: Uuid, finished_at: DateTime<Utc>) -> Result<(), DomainError>;

    /// Moves a RUNNING run to FAILED at `finished_at` with a failure message.
    fn fail(&self, id: Uuid, finished_at: DateTime<Utc>, message: &str) -> Result<(), DomainError>;

    /// Moves a RUNNING run to FINISHED at `finished_at`, snapshotting the run's
    /// import outcome counters. The default delegates to [`finish`](Self::finish)
    /// and drops the facts so in-memory test doubles keep compiling.
    fn finish_with_facts(
        &self,
        id: Uuid,
        finished_at: DateTime<Utc>,
        _facts: ImportRunFacts,
    ) -> Result<(), DomainError> {
        self.finish(id, finished_at)
    }

    /// Moves a RUNNING run to FAILED at `finished_at` with a message and the
    /// run's outcome counters. The default delegates to [`fail`](Self::fail).
    fn fail_with_facts(
        &self,
        id: Uuid,
        finished_at: DateTime<Utc>,
        message: &str,
        _facts: ImportRunFacts,
    ) -> Result<(), DomainError> {
        self.fail(id, finished_at, message)
    }

    /// The newest run of a data source (by `started_at`), if any.
    fn latest_by_data_source(
        &self,
        data_source_id: DataSourceId,
    ) -> Result<Option<DataImportRun>, DomainError>;

    /// All runs of a data source, newest first (by `started_at`). The default
    /// returns at most the latest run so in-memory test doubles keep compiling;
    /// the Postgres adapter returns the full history.
    fn list_by_data_source(
        &self,
        data_source_id: DataSourceId,
    ) -> Result<Vec<DataImportRun>, DomainError> {
        Ok(self
            .latest_by_data_source(data_source_id)?
            .into_iter()
            .collect())
    }

    /// One newest-first page of a data source's runs plus the total count and
    /// the average duration over all finished runs. The default slices
    /// [`Self::list_by_data_source`] so in-memory test doubles keep compiling;
    /// the Postgres adapter pushes the window into SQL.
    fn list_page_by_data_source(
        &self,
        data_source_id: DataSourceId,
        offset: usize,
        limit: usize,
    ) -> Result<ImportRunPage, DomainError> {
        let all = self.list_by_data_source(data_source_id)?;
        let total = all.len() as u64;
        let finished_durations: Vec<i64> = all
            .iter()
            .filter(|run| run.status == ImportRunStatus::Finished)
            .filter_map(DataImportRun::duration_seconds)
            .collect();
        let average_duration_seconds = if finished_durations.is_empty() {
            None
        } else {
            Some(finished_durations.iter().sum::<i64>() / finished_durations.len() as i64)
        };
        Ok(ImportRunPage {
            items: all.into_iter().skip(offset).take(limit).collect(),
            total,
            average_duration_seconds,
        })
    }

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

#[cfg(test)]
mod tests {
    use std::sync::Mutex;

    use super::*;

    struct Recording {
        finished: Mutex<Vec<Uuid>>,
        failed: Mutex<Vec<String>>,
    }

    impl DataImportRunRepository for Recording {
        fn insert(&self, _run: &DataImportRun) -> Result<(), DomainError> {
            Ok(())
        }

        fn finish(&self, id: Uuid, _finished_at: DateTime<Utc>) -> Result<(), DomainError> {
            self.finished.lock().unwrap().push(id);
            Ok(())
        }

        fn fail(
            &self,
            _id: Uuid,
            _finished_at: DateTime<Utc>,
            message: &str,
        ) -> Result<(), DomainError> {
            self.failed.lock().unwrap().push(message.to_string());
            Ok(())
        }

        fn latest_by_data_source(
            &self,
            _data_source_id: DataSourceId,
        ) -> Result<Option<DataImportRun>, DomainError> {
            Ok(None)
        }

        fn finalize_orphaned_running(
            &self,
            _older_than: DateTime<Utc>,
        ) -> Result<u64, DomainError> {
            Ok(0)
        }
    }

    #[test]
    fn update_progress_default_is_a_no_op() {
        let repo = Recording {
            finished: Mutex::new(Vec::new()),
            failed: Mutex::new(Vec::new()),
        };
        repo.update_progress(Uuid::new_v4(), ImportRunFacts::default())
            .unwrap();
    }

    #[test]
    fn finish_with_facts_delegates_to_finish() {
        let repo = Recording {
            finished: Mutex::new(Vec::new()),
            failed: Mutex::new(Vec::new()),
        };
        let id = Uuid::new_v4();
        repo.finish_with_facts(id, Utc::now(), ImportRunFacts::default())
            .unwrap();
        assert_eq!(repo.finished.lock().unwrap().as_slice(), &[id]);
    }

    #[test]
    fn fail_with_facts_delegates_to_fail() {
        let repo = Recording {
            finished: Mutex::new(Vec::new()),
            failed: Mutex::new(Vec::new()),
        };
        repo.fail_with_facts(
            Uuid::new_v4(),
            Utc::now(),
            "boom",
            ImportRunFacts::default(),
        )
        .unwrap();
        assert_eq!(
            repo.failed.lock().unwrap().as_slice(),
            &["boom".to_string()]
        );
    }

    #[test]
    fn list_by_data_source_defaults_to_latest() {
        let repo = Recording {
            finished: Mutex::new(Vec::new()),
            failed: Mutex::new(Vec::new()),
        };
        let runs = repo
            .list_by_data_source(DataSourceId(Uuid::new_v4()))
            .unwrap();
        assert!(runs.is_empty());
    }
}
