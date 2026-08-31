-- Wall-clock time a data source was last successfully updated. Drives the UI
-- "last updated" timestamps and, unlike the coarse update job (which is FAILED
-- when any single source fails), survives partial multi-source runs.
--
-- Note: the ORIGINAL `last_updated_at` column (added in V3) was renamed to
-- `imported_until` by V7, so this adds a fresh column with the same name for
-- the per-source wall-clock marker.
ALTER TABLE data_sources ADD COLUMN last_updated_at TIMESTAMPTZ;
