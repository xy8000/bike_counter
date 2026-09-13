-- Pre-aggregated hourly and daily rollups over the raw measurements, keyed by
-- the channel's own counting-station local calendar bucket. The analytics read
-- model sums these instead of scanning the raw history for coarse windows
-- (overview metrics, monthly bar, 30-day/year graphs, hour-of-day radars).
--
-- `local_date` / `local_hour` are computed in the channel's own station timezone
-- (each channel belongs to exactly one counting station with one timezone), which
-- matches the analytics' DST-aware local-day/local-hour semantics.
--
-- This migration is DDL only: it creates the (empty) rollup tables and indexes.
-- The one-time backfill of the existing history runs in the background in the
-- `measurement_rollup` job (which reads the measurement bounds and refreshes the
-- whole range on its first run) instead of blocking startup with a full scan of
-- the measurement history.

CREATE TABLE measurement_hourly (
    channel_id         UUID     NOT NULL REFERENCES channels(id) ON DELETE CASCADE,
    resolution_seconds BIGINT   NOT NULL,
    local_date         DATE     NOT NULL,
    local_hour         SMALLINT NOT NULL,
    total              BIGINT   NOT NULL,
    PRIMARY KEY (channel_id, resolution_seconds, local_date, local_hour)
);

CREATE TABLE measurement_daily (
    channel_id         UUID   NOT NULL REFERENCES channels(id) ON DELETE CASCADE,
    resolution_seconds BIGINT NOT NULL,
    local_date         DATE   NOT NULL,
    total              BIGINT NOT NULL,
    PRIMARY KEY (channel_id, resolution_seconds, local_date)
);

-- Date-first indexes for the read path, which filters a channel list by a local
-- date range and then groups by month or hour-of-day.
CREATE INDEX measurement_hourly_date_resolution_idx
    ON measurement_hourly (local_date, resolution_seconds);
CREATE INDEX measurement_daily_date_resolution_idx
    ON measurement_daily (local_date, resolution_seconds);
