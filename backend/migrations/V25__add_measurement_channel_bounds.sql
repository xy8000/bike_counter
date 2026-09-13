-- Per-channel measurement bounds and the rollup readiness flag.
--
-- The Bike-Trends "exclude new stations" filter needs each channel's
-- earliest-ever measurement. Answering that from the raw table with
-- `DISTINCT ON (channel_id) ... ORDER BY channel_id, timestamp` forces Postgres
-- to walk every row of every requested channel (there is no loose index scan),
-- which reintroduced the large raw scans the hourly/daily rollups removed.
-- `measurement_channel_bounds` stores the first/last timestamp per channel, is
-- maintained by the same `measurement_rollup` job that maintains the rollups, and
-- turns the predicate into a single indexed row lookup per channel.
--
-- Both objects are DDL only: the one-time backfill runs in the job, never in the
-- migration, so startup never blocks on a full history scan.

CREATE TABLE measurement_channel_bounds (
    channel_id      UUID        PRIMARY KEY REFERENCES channels(id) ON DELETE CASCADE,
    first_timestamp TIMESTAMPTZ NOT NULL,
    last_timestamp  TIMESTAMPTZ NOT NULL
);

-- Whether the one-time rollup backfill has completed. Until it has, the
-- analytics read the raw table (the pre-rollup behaviour) so a fresh deploy
-- serves correct numbers instead of zeros while the backfill is still running.
-- Single-row table (id is pinned to 1 by the check constraint).
CREATE TABLE measurement_rollup_state (
    id         SMALLINT PRIMARY KEY DEFAULT 1 CHECK (id = 1),
    backfilled BOOLEAN  NOT NULL DEFAULT FALSE
);

INSERT INTO measurement_rollup_state (id, backfilled) VALUES (1, FALSE);
