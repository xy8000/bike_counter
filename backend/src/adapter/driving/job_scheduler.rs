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
            tracing::error!("Invalid cron expression '{cron_expression}': {error}");
            return;
        }
    };

    loop {
        let now = Utc::now();
        let Some(next) = schedule.after(&now).next() else {
            tracing::error!("Cron schedule produced no future triggers; stopping scheduler");
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

#[cfg(test)]
mod tests {
    use std::sync::Arc;
    use std::sync::atomic::{AtomicUsize, Ordering};

    use super::*;

    /// A scheduled job that only counts how often the scheduler invoked it.
    struct CountingJob {
        calls: Arc<AtomicUsize>,
    }

    impl ScheduledJobPort for CountingJob {
        fn run_if_due(&self) {
            self.calls.fetch_add(1, Ordering::SeqCst);
        }
    }

    #[test]
    fn run_scheduler_invokes_the_job_once_at_startup() {
        // Regression guard for "an overdue data-source update never starts": the
        // scheduler must run the job immediately at startup, before any cron
        // tick. An invalid expression then stops the loop deterministically.
        let calls = Arc::new(AtomicUsize::new(0));
        let job: Arc<dyn ScheduledJobPort> = Arc::new(CountingJob {
            calls: calls.clone(),
        });

        let runtime = tokio::runtime::Runtime::new().unwrap();
        runtime.block_on(run_scheduler(job, "not a cron".to_string()));

        assert_eq!(
            calls.load(Ordering::SeqCst),
            1,
            "the scheduler must call run_if_due once at startup"
        );
    }
}
