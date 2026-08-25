//! Generic driving (inbound) port for **scheduled** jobs, implemented by every
//! job service the cron scheduler drives (`DataSourceUpdateService` and
//! `AssetCleanupService`). The scheduler runs `run_if_due` on a blocking thread
//! at startup and on each CRON tick; each service decides whether to run (never
//! succeeded or overdue) and tracks itself as a ShedLock-style job.

pub trait ScheduledJobPort: Send + Sync {
    fn run_if_due(&self);
}
