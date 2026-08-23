//! Async cron scheduler that periodically triggers the data-source update job.
//!
//! The blocking repository calls inside `run_if_due` must not run on a tokio
//! worker thread (the synchronous `postgres` crate panics on nested runtimes),
//! so every invocation is wrapped in `tokio::task::spawn_blocking`.

use std::str::FromStr;
use std::sync::Arc;
use std::time::Duration as StdDuration;

use chrono::Utc;

use crate::core::application::data_source_update_service::DataSourceUpdateService;
use crate::core::domain::configuration::configuration::Configuration;

/// Runs the data-source update job immediately at startup and then on the
/// configured CRON schedule, forever. The job service decides whether to run
/// (never succeeded or overdue).
pub async fn run_scheduler(
    service: Arc<DataSourceUpdateService>,
    configuration: Arc<Configuration>,
) {
    // Run now: the job service starts the job only when it has never succeeded
    // or the last successful run is overdue (missed cron triggers).
    let service_for_startup = service.clone();
    let _ = tokio::task::spawn_blocking(move || service_for_startup.run_if_due()).await;

    let schedule = match cron::Schedule::from_str(configuration.data_source_update_cron()) {
        Ok(schedule) => schedule,
        Err(error) => {
            eprintln!("Invalid data_source_update_cron: {error}");
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
