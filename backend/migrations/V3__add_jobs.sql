-- ShedLock-style generic job tracking.
-- lifetime_until is an absolute deadline timestamp (TIMESTAMPTZ): a RUNNING job
-- only "lives" before this timestamp. It is NOT NULL and has NO default: a job
-- must be inserted with an explicit deadline or the insert fails at the
-- database level (mirrors the domain rule).
CREATE TABLE jobs (
    id UUID PRIMARY KEY,
    name TEXT NOT NULL,
    job_type TEXT NOT NULL,
    status TEXT NOT NULL DEFAULT 'PENDING'
        CHECK (status IN ('PENDING', 'RUNNING', 'FINISHED', 'FAILED')),
    started_at TIMESTAMPTZ,
    finished_at TIMESTAMPTZ,
    failure_message TEXT,
    metadata JSONB NOT NULL DEFAULT '{}'::jsonb,
    lifetime_until TIMESTAMPTZ NOT NULL,
    max_lifetime_exceeded BOOLEAN NOT NULL DEFAULT FALSE,
    created_at TIMESTAMPTZ NOT NULL DEFAULT now()
);

CREATE INDEX idx_jobs_type_status ON jobs (job_type, status);
CREATE INDEX idx_jobs_status ON jobs (status);

-- Incremental data-source update marker (database-only, not exposed via the API).
ALTER TABLE data_sources ADD COLUMN last_updated_at TIMESTAMPTZ;
