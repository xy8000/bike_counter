-- Per-run import facts for the data-source jobs overview (GitHub issue #38).
--
-- The four counters are snapshotted when a run finishes so the UI can list each
-- run's outcome directly instead of re-deriving it from provider messages (which
-- are only countable "since a timestamp", not per run) or the aggregate job
-- metadata (which only keeps the running/last totals, not the history).
ALTER TABLE data_source_imports
    ADD COLUMN added_measurements BIGINT NOT NULL DEFAULT 0,
    ADD COLUMN added_stations BIGINT NOT NULL DEFAULT 0,
    ADD COLUMN warning_count BIGINT NOT NULL DEFAULT 0,
    ADD COLUMN error_count BIGINT NOT NULL DEFAULT 0;
