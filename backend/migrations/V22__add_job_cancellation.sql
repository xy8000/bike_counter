-- Multi-instance job cancellation: replace the single-instance ShedLock
-- lifetime-deadline model with per-instance ownership (`instance_id`) plus a
-- heartbeat (`heartbeat_at`) and a two-phase cancellation status, backed by a
-- dedicated ShedLock-style `job_locks` table that makes the claim atomic.
--
-- The transient PENDING status is removed: a job row is only ever created
-- after its owning instance has acquired the `job_locks` row, so it is inserted
-- directly as RUNNING.

-- Ownership/liveness columns (nullable so pre-migration rows need no backfill;
-- the watcher treats a NULL heartbeat as stale and cancels orphaned rows).
ALTER TABLE jobs
    DROP COLUMN lifetime_until,
    DROP COLUMN max_lifetime_exceeded,
    ADD COLUMN instance_id UUID,
    ADD COLUMN heartbeat_at TIMESTAMPTZ;

-- A pre-migration PENDING row can never be claimed under the new model
-- (claiming now happens on job_locks before a job row exists), so cancel it.
UPDATE jobs
SET status = 'CANCELLED',
    failure_message = 'abandoned before ownership',
    finished_at = now()
WHERE status = 'PENDING';

ALTER TABLE jobs DROP CONSTRAINT jobs_status_check;
ALTER TABLE jobs ADD CONSTRAINT jobs_status_check
    CHECK (status IN ('RUNNING', 'FINISHED', 'FAILED',
                      'CANCELLATION_REQUESTED', 'CANCELLED'));

CREATE INDEX idx_jobs_type_status_heartbeat ON jobs (job_type, status, heartbeat_at);

-- ShedLock-style mutual exclusion. Acquire is a single atomic
-- `INSERT ... ON CONFLICT (job_type) DO UPDATE ... WHERE lock_until < now()`;
-- the lock persists for the whole job life and is released at the terminal
-- transition.
CREATE TABLE job_locks (
    job_type TEXT PRIMARY KEY,
    locked_by UUID NOT NULL,
    lock_until TIMESTAMPTZ NOT NULL
);
