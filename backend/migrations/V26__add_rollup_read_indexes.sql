-- Covering indexes for the hourly/daily rollup read path.
--
-- The rollup reads filter `channel_id = ANY(...)` plus a local-date range but
-- leave `resolution_seconds` unconstrained (the analytics sum every resolution).
-- The primary keys lead with `channel_id` but then `resolution_seconds`, so with
-- the resolution unconstrained Postgres cannot use the local-date range and ends
-- up scanning each requested channel's entire history instead.
--
-- Measured on the production-sized dev data, the summary's year/weekly request
-- read ~1.3 GB from disk: the hour radar alone read ~815 MB (104 k blocks) and
-- took ~5.7 s because it walked every channel's whole hourly history to keep one
-- year of it.
--
-- A `(channel_id, local_date)` index turns that into a per-channel date range
-- scan, and the INCLUDE columns make it an index-only scan for the aggregate
-- SELECTs (which read the grouping keys plus `total`). Both rollup tables are
-- written only by the rollup job and are far smaller than the raw history, so the
-- extra indexes are cheap to maintain. Measured effect: the same hour radar drops
-- to ~52 MB / 0.32 s and the whole year/weekly request to ~12 MB / 0.56 s.
--
-- DDL only. Unlike `V24` these have no backfill scan; a plain CREATE INDEX over
-- the already-populated rollup tables is a few seconds.
CREATE INDEX measurement_hourly_channel_date_idx
    ON measurement_hourly (channel_id, local_date)
    INCLUDE (resolution_seconds, local_hour, total);

CREATE INDEX measurement_daily_channel_date_idx
    ON measurement_daily (channel_id, local_date)
    INCLUDE (resolution_seconds, total);
