-- Resolution dimension for measurements.
--
-- Every measurement now knows the length (in seconds) of the interval its count
-- covers, so the analytics can combine sources that publish the same counter at
-- different resolutions (e.g. Hamburg's 5-min / 15-min / hourly / daily
-- datastreams) without double counting.
--
-- There is intentionally NO default: a measurement with an unknown resolution is
-- invalid. The two pre-existing sources are backfilled explicitly by data-source
-- name; any row that is still NULL afterwards is dropped (a re-import recovers
-- it) rather than being silently assigned a wrong duration.
--
-- The overlap guard (measurements_no_overlap) rejects same-resolution rows whose
-- intervals intersect in a channel (e.g. a 60-second row followed by one a second
-- later). It is implemented as a btree index + BEFORE INSERT/UPDATE trigger, not
-- as a GiST exclusion constraint: building a GiST index over a large history is
-- roughly 30x slower than a btree build (measured on 1M rows), which made the
-- migration impractical on big tables. The trigger enforces the same rule for
-- every insert/update while the btree index keeps the per-row check cheap.
-- `interval_end` stays nullable: it is only set for calendar-anchored resolutions
-- (daily/weekly, DST-aware); fixed-second resolutions leave it NULL and the guard
-- derives the end as `timestamp + resolution_seconds`.

ALTER TABLE measurements ADD COLUMN resolution_seconds BIGINT;

-- Münster publishes 15-minute counts.
UPDATE measurements m
SET resolution_seconds = 900
WHERE m.channel_id IN (
    SELECT c.id FROM channels c
    JOIN counting_stations s ON s.id = c.counting_station_id
    JOIN data_sources d ON d.id = s.data_source_id
    WHERE d.name = 'Münster'
);

-- Bonn publishes hourly counts.
UPDATE measurements m
SET resolution_seconds = 3600
WHERE m.channel_id IN (
    SELECT c.id FROM channels c
    JOIN counting_stations s ON s.id = c.counting_station_id
    JOIN data_sources d ON d.id = s.data_source_id
    WHERE d.name = 'Bonn'
);

-- Any row with an unknown resolution cannot be served correctly; drop it.
DELETE FROM measurements WHERE resolution_seconds IS NULL;

ALTER TABLE measurements ALTER COLUMN resolution_seconds SET NOT NULL;
ALTER TABLE measurements ADD CONSTRAINT measurements_resolution_positive
    CHECK (resolution_seconds > 0);

-- A channel may carry several resolutions, so the natural key widens. The
-- leading (channel_id) column keeps the index usable for channel-filtered
-- pagination and idempotent upserts.
ALTER TABLE measurements DROP CONSTRAINT measurements_channel_id_timestamp_key;
ALTER TABLE measurements ADD CONSTRAINT measurements_channel_timestamp_resolution_key
    UNIQUE (channel_id, timestamp, resolution_seconds);

-- Exact interval end for calendar-anchored resolutions (daily/weekly), set
-- DST-aware by the adapter; NULL for fixed-second resolutions (derived on the
-- fly). No backfill, so this migration never rewrites the whole table.
ALTER TABLE measurements ADD COLUMN interval_end TIMESTAMPTZ;

-- Cheap index backing the overlap guard: equality on (channel_id,
-- resolution_seconds) plus a `timestamp` range bounds every overlap check.
CREATE INDEX measurements_channel_resolution_time_idx
    ON measurements (channel_id, resolution_seconds, timestamp);

-- Overlap guard: rejects any row whose interval intersects an existing row of
-- the same channel at the same resolution. Adjacent (back-to-back) intervals are
-- allowed; different resolutions at the same timestamp are allowed; a row with
-- the identical natural key is left to the UNIQUE constraint / ON CONFLICT.
CREATE OR REPLACE FUNCTION measurements_no_overlap_guard() RETURNS trigger
LANGUAGE plpgsql AS $$
DECLARE
    stop_ts timestamptz := COALESCE(
        NEW.interval_end,
        NEW.timestamp + NEW.resolution_seconds * interval '1 second'
    );
BEGIN
    IF EXISTS (
        SELECT 1 FROM measurements m
        WHERE m.channel_id = NEW.channel_id
          AND m.resolution_seconds = NEW.resolution_seconds
          AND m.timestamp < stop_ts
          AND COALESCE(
                  m.interval_end,
                  m.timestamp + m.resolution_seconds * interval '1 second'
              ) > NEW.timestamp
          AND NOT (
              m.timestamp = NEW.timestamp
              AND m.resolution_seconds = NEW.resolution_seconds
          )
    ) THEN
        RAISE EXCEPTION 'overlapping measurement interval (channel %, resolution %, timestamp %)',
            NEW.channel_id, NEW.resolution_seconds, NEW.timestamp
            USING ERRCODE = 'check_violation';
    END IF;
    RETURN NEW;
END $$;

DROP TRIGGER IF EXISTS measurements_no_overlap ON measurements;
CREATE TRIGGER measurements_no_overlap
BEFORE INSERT OR UPDATE OF timestamp, interval_end, resolution_seconds
ON measurements FOR EACH ROW
EXECUTE FUNCTION measurements_no_overlap_guard();
