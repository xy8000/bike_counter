-- Per-source import runs. The aggregate `data_source_update` job updates every
-- configured data source in a single run, but each source now also records its
-- own run so the data-sources UI can show per-source last-import facts: the
-- status (success/failure), the duration (finished_at - started_at) and a
-- failure message. Warning/error counts are derived on read by counting the
-- provider messages recorded since the run started.
CREATE TABLE data_source_imports (
    id UUID PRIMARY KEY,
    data_source_id UUID NOT NULL REFERENCES data_sources(id) ON DELETE CASCADE,
    job_id UUID REFERENCES jobs(id) ON DELETE SET NULL,
    started_at TIMESTAMPTZ NOT NULL,
    finished_at TIMESTAMPTZ,
    status TEXT NOT NULL CHECK (status IN ('RUNNING', 'FINISHED', 'FAILED')),
    failure_message TEXT,
    created_at TIMESTAMPTZ NOT NULL DEFAULT now()
);

-- The UI reads the newest run per data source; keep that lookup indexed.
CREATE INDEX idx_data_source_imports_source_started
    ON data_source_imports (data_source_id, started_at DESC);
