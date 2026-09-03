-- Covering index for the time-windowed SUM aggregates (the header global
-- summary, sidebar/summary `bikes_last_day`, graph windows).
--
-- Those aggregates filter on a `timestamp` range (e.g. the previous local
-- day) across many or all channels, so an index leading on `channel_id` (the
-- natural key and the overlap-guard index) cannot bound the range and Postgres
-- falls back to a sequential scan over the whole history. A `timestamp`-leading
-- index turns the window into a tight range scan, and the INCLUDE columns let
-- Postgres answer `SUM(value) GROUP BY channel_id` without heap lookups
-- (index-only scan). `interval_end` is not needed by any read path.
CREATE INDEX measurements_timestamp_idx
    ON measurements (timestamp)
    INCLUDE (channel_id, resolution_seconds, value);
