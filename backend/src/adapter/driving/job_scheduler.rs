//! Async cron scheduler that periodically triggers a scheduled job
//! ([`ScheduledJobPort`], e.g. the data-source update or the asset cleanup job).
//!
//! The blocking repository calls inside `run_if_due` must not run on a tokio
//! worker thread (the synchronous `postgres` crate panics on nested runtimes),
//! so every invocation is wrapped in `tokio::task::spawn_blocking`.

use std::str::FromStr;
use std::sync::Arc;
use std::time::Duration as StdDuration;

use chrono::Utc;

use crate::core::application::job_reconciliation_service::JobReconciliationService;
use crate::core::domain::jobs::scheduled_job_port::ScheduledJobPort;

/// Runs `service` immediately at startup and then on the given CRON schedule,
/// forever. The service decides whether to run (never succeeded or overdue).
pub async fn run_scheduler(service: Arc<dyn ScheduledJobPort>, cron_expression: String) {
    // Run now: the service starts the job only when it has never succeeded or
    // the last successful run is overdue (missed cron triggers).
    let service_for_startup = service.clone();
    let _ = tokio::task::spawn_blocking(move || service_for_startup.run_if_due()).await;

    let schedule = match cron::Schedule::from_str(&cron_expression) {
        Ok(schedule) => schedule,
        Err(error) => {
            eprintln!("Invalid cron expression '{cron_expression}': {error}");
            return;
        }
    };

    loop {
        let now = Utc::now();
        let Some(next) = schedule.after(&now).next() else {
            eprintln!("Cron schedule produced no future triggers; stopping scheduler");
            return;
        };

        let delay = (next - now).to_std().unwrap_or(StdDuration::from_secs(1));
        tokio::time::sleep(delay).await;

        let service_for_tick = service.clone();
        let _ = tokio::task::spawn_blocking(move || service_for_tick.run_if_due()).await;
    }
}

/// Runs the job watcher forever on a fixed short interval: it reconciles stale
/// active jobs of every scheduled type (heartbeat-based) so cancelled or
/// crashed workers are finalized promptly, independently of the (possibly
/// sparse) cron schedules of the individual job types. The blocking repository
/// calls run on the tokio blocking pool (see the module docs).
pub async fn run_job_watcher(service: Arc<JobReconciliationService>, interval: StdDuration) {
    loop {
        let service_for_tick = service.clone();
        let _ =
            tokio::task::spawn_blocking(move || service_for_tick.reconcile_all(Utc::now())).await;
        tokio::time::sleep(interval).await;
    }
}
