//! Dedicated per-job heartbeat loop.
//!
//! A job's liveness must not depend on sub-task boundaries (an import batch or
//! the atomic tiles build can outlast the heartbeat interval). While a job runs,
//! the owning service starts a [`JobHeartbeat`]: a short-lived OS thread that
//! refreshes the job's `heartbeat_at` (and extends the type's `job_locks` lease)
//! on a fixed tick — at least three beats per interval, capped at 5 s. It also
//! observes the returned status and sets a shared cancellation flag as soon as
//! the job is no longer RUNNING, so the worker gets prompt cancellation
//! detection bounded by the tick rather than by sub-task duration.

use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use std::thread;
use std::time::Duration as StdDuration;

use chrono::Utc;
use uuid::Uuid;

use crate::core::domain::jobs::job::JobStatus;
use crate::core::domain::jobs::repository_port::JobRepository;

pub struct JobHeartbeat {
    stop: Arc<AtomicBool>,
    handle: thread::JoinHandle<()>,
    cancelled: Arc<AtomicBool>,
}

impl JobHeartbeat {
    /// Starts the heartbeat loop for `job_id`. `interval` is the type's max
    /// heartbeat interval; the loop beats roughly every
    /// `min(interval / 3, 5 s)` so the watcher sees at least three fresh beats
    /// per interval. `heartbeat` is owner-only, so only this instance can keep
    /// the job alive.
    pub fn start(
        job_repository: Arc<dyn JobRepository + Send + Sync>,
        job_id: Uuid,
        job_type: &'static str,
        instance_id: Uuid,
        interval: chrono::Duration,
    ) -> Self {
        let stop = Arc::new(AtomicBool::new(false));
        let cancelled = Arc::new(AtomicBool::new(false));

        let stop_thread = stop.clone();
        let cancelled_thread = cancelled.clone();
        // Beat every interval/3 (floor 1 s, ceiling 5 s).
        let tick_secs = (interval.num_seconds() / 3).clamp(1, 5) as u64;
        let tick = StdDuration::from_secs(tick_secs);

        let handle = thread::spawn(move || {
            loop {
                if stop_thread.load(Ordering::Relaxed) {
                    return;
                }
                let now = Utc::now();
                match job_repository.heartbeat(
                    job_id,
                    job_type,
                    instance_id,
                    now,
                    now + interval,
                ) {
                    // Still RUNNING: keep beating.
                    Ok(JobStatus::Running) => {}
                    // Cancellation requested / already cancelled / terminal: signal
                    // the worker and exit.
                    Ok(_) => {
                        cancelled_thread.store(true, Ordering::Relaxed);
                        return;
                    }
                    // A transient failure is not fatal: try again next tick.
                    Err(_) => {}
                }
                // Sleep in small increments so `stop` is honoured promptly.
                let mut slept = StdDuration::ZERO;
                while slept < tick {
                    if stop_thread.load(Ordering::Relaxed) {
                        return;
                    }
                    let step = StdDuration::from_millis(100);
                    thread::sleep(step);
                    slept += step;
                }
            }
        });

        Self {
            stop,
            handle,
            cancelled,
        }
    }

    /// A clone of the shared cancellation flag the worker checks at sub-task
    /// boundaries (set when the job is no longer RUNNING).
    pub fn cancelled_flag(&self) -> Arc<AtomicBool> {
        self.cancelled.clone()
    }

    /// Signals the loop to stop and joins it. Call after the job work finishes,
    /// before the terminal transition, so no further writes race `finalize`.
    pub fn stop(self) {
        self.stop.store(true, Ordering::Relaxed);
        let _ = self.handle.join();
    }
}

#[cfg(test)]
mod tests {
    use std::sync::{Arc, Mutex};

    use chrono::{DateTime, Duration, Utc};
    use serde_json::Value;
    use uuid::Uuid;

    use super::JobHeartbeat;
    use crate::core::domain::error::DomainError;
    use crate::core::domain::jobs::job::{Job, JobStatus};
    use crate::core::domain::jobs::repository_port::JobRepository;

    const JOB_TYPE: &str = "test_type";
    const INSTANCE: Uuid = Uuid::from_u128(0x0000_0000_0000_0000_0000_0000_0000_00D1);

    /// Records heartbeats into a shared counter; can be told to report a
    /// cancellation request after a given number of beats.
    struct RecordingRepo {
        beats: Arc<Mutex<Vec<DateTime<Utc>>>>,
        cancel_after: usize,
    }

    impl JobRepository for RecordingRepo {
        fn insert(&self, _job: Job) -> Result<(), DomainError> {
            Ok(())
        }

        fn acquire(
            &self,
            _job_type: &str,
            _instance_id: Uuid,
            _lock_until: DateTime<Utc>,
        ) -> Result<bool, DomainError> {
            Ok(true)
        }

        fn release(&self, _job_type: &str, _instance_id: Uuid) -> Result<(), DomainError> {
            Ok(())
        }

        fn heartbeat(
            &self,
            _id: Uuid,
            _job_type: &str,
            _instance_id: Uuid,
            at: DateTime<Utc>,
            _lock_until: DateTime<Utc>,
        ) -> Result<JobStatus, DomainError> {
            let mut beats = self.beats.lock().unwrap();
            beats.push(at);
            if beats.len() > self.cancel_after {
                Ok(JobStatus::CancellationRequested)
            } else {
                Ok(JobStatus::Running)
            }
        }

        fn set_finished(&self, _id: Uuid, _finished_at: DateTime<Utc>) -> Result<(), DomainError> {
            Ok(())
        }

        fn set_failed(
            &self,
            _id: Uuid,
            _finished_at: DateTime<Utc>,
            _message: &str,
        ) -> Result<(), DomainError> {
            Ok(())
        }

        fn request_cancellation(&self, _id: Uuid) -> Result<(), DomainError> {
            Ok(())
        }

        fn mark_cancelled(&self, _id: Uuid, _finished_at: DateTime<Utc>) -> Result<(), DomainError> {
            Ok(())
        }

        fn update_metadata(&self, _id: Uuid, _key: &str, _value: Value) -> Result<(), DomainError> {
            Ok(())
        }

        fn find_by_id(&self, _id: Uuid) -> Result<Option<Job>, DomainError> {
            Ok(None)
        }

        fn find_all(
            &self,
            _job_type: Option<&str>,
            _status: Option<JobStatus>,
        ) -> Result<Vec<Job>, DomainError> {
            Ok(Vec::new())
        }

        fn find_active_by_type(&self, _job_type: &str) -> Result<Vec<Job>, DomainError> {
            Ok(Vec::new())
        }

        fn find_last_finished_by_type(&self, _job_type: &str) -> Result<Option<Job>, DomainError> {
            Ok(None)
        }

        fn reconcile_stale_active(
            &self,
            _job_type: &str,
            _heartbeat_before: DateTime<Utc>,
            _now: DateTime<Utc>,
        ) -> Result<(), DomainError> {
            Ok(())
        }
    }

    #[test]
    fn heartbeats_repeatedly_and_flags_cancellation() {
        let beats = Arc::new(Mutex::new(Vec::new()));
        let repo = Arc::new(RecordingRepo {
            beats: beats.clone(),
            cancel_after: 2,
        });
        let job_id = Uuid::new_v4();
        let interval = Duration::seconds(3); // tick = 1 s

        let heartbeat = JobHeartbeat::start(repo, job_id, JOB_TYPE, INSTANCE, interval);
        let cancelled = heartbeat.cancelled_flag();

        // The loop beats until the repo reports CANCELLATION_REQUESTED (beat 3),
        // then sets the flag and exits on its own.
        let deadline = Utc::now() + Duration::seconds(10);
        while !cancelled.load(std::sync::atomic::Ordering::Relaxed) && Utc::now() < deadline {
            std::thread::sleep(Duration::milliseconds(50).to_std().unwrap());
        }
        assert!(
            cancelled.load(std::sync::atomic::Ordering::Relaxed),
            "the heartbeat loop must set the cancellation flag"
        );
        // No need to stop() — the loop already exited on the cancellation.
        heartbeat.stop();

        let recorded = beats.lock().unwrap().len();
        assert!(recorded >= 3, "expected at least 3 beats, got {recorded}");
    }
}
