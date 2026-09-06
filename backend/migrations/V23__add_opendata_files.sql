-- OpenData export file registry: one row per generated, immutable distribution
-- file (parquet / csv.gz / json) for a global or per-station daily/monthly
-- period. The registry is both the append-only ledger and the export job's
-- state: the job computes missing periods by comparing the available measurement
-- periods with already-registered files and never overwrites an existing row.
CREATE TABLE opendata_files (
    id UUID PRIMARY KEY,
    -- Full object key in the opendata bucket, e.g.
    -- opendata/measurements/daily/2026/2026-09-05.parquet
    -- opendata/stations/{station_id}/measurements/monthly/2026-09/2026-09.json
    object_key TEXT NOT NULL UNIQUE,
    -- NULL for global files; the counting-station UUID for per-station files.
    station_id UUID,
    granularity TEXT NOT NULL CHECK (granularity IN ('daily', 'monthly')),
    -- 'YYYY-MM-DD' for daily, 'YYYY-MM' for monthly (zero-padded and sortable).
    period TEXT NOT NULL,
    format TEXT NOT NULL CHECK (format IN ('parquet', 'csv.gz', 'json')),
    byte_size BIGINT NOT NULL,
    sha256 TEXT NOT NULL,
    created_at TIMESTAMPTZ NOT NULL DEFAULT now()
);

-- DB-level append-only guards: exactly one file per (scope, granularity,
-- period, format). Partial indexes keep NULL station_id (global) distinct from
-- per-station rows.
CREATE UNIQUE INDEX uq_opendata_files_global
    ON opendata_files (granularity, period, format) WHERE station_id IS NULL;
CREATE UNIQUE INDEX uq_opendata_files_station
    ON opendata_files (station_id, granularity, period, format)
    WHERE station_id IS NOT NULL;

-- Index/period lookups for the REST endpoints.
CREATE INDEX idx_opendata_files_station_period
    ON opendata_files (station_id, granularity, period);
CREATE INDEX idx_opendata_files_global_period
    ON opendata_files (granularity, period) WHERE station_id IS NULL;
