-- Optimize the measurements overlap guard introduced in V14.
--
-- V14's version ran an `EXISTS` predicate for every inserted/updated row with
-- an unbounded `m.timestamp < stop_ts` range plus a non-indexable derived
-- interval-end term. On a populated table Postgres scans forward through the
-- whole (channel_id, resolution_seconds) history to find the first matching
-- row, making each insert O(history) and a multi-row import roughly quadratic.
--
-- Because the guard itself keeps intervals non-overlapping per
-- (channel_id, resolution_seconds), interval ends are monotonically
-- non-decreasing with timestamp. It is therefore sufficient to check only the
-- row with the greatest timestamp below the new interval end: if that row does
-- not overlap, no earlier row can. That single lookup is served by the existing
-- btree index `measurements_channel_resolution_time_idx`
-- (channel_id, resolution_seconds, timestamp) via ORDER BY timestamp DESC
-- LIMIT 1, turning each row check into O(log n).
--
-- The trigger already references the function by name, so replacing the
-- function is enough; no trigger drop/recreate is needed. Error message and
-- ERRCODE stay identical, and the NOT (...) filter still lets ON CONFLICT
-- DO NOTHING swallow idempotent re-inserts of identical rows.
CREATE OR REPLACE FUNCTION measurements_no_overlap_guard() RETURNS trigger
LANGUAGE plpgsql AS $$
DECLARE
    stop_ts timestamptz := COALESCE(
        NEW.interval_end,
        NEW.timestamp + NEW.resolution_seconds * interval '1 second'
    );
    predecessor timestamptz;
    predecessor_stop timestamptz;
BEGIN
    SELECT m.timestamp,
           COALESCE(
               m.interval_end,
               m.timestamp + m.resolution_seconds * interval '1 second'
           )
      INTO predecessor, predecessor_stop
      FROM measurements m
     WHERE m.channel_id = NEW.channel_id
       AND m.resolution_seconds = NEW.resolution_seconds
       AND m.timestamp < stop_ts
       AND NOT (
           m.timestamp = NEW.timestamp
           AND m.resolution_seconds = NEW.resolution_seconds
       )
     ORDER BY m.timestamp DESC
     LIMIT 1;

    IF FOUND AND predecessor_stop > NEW.timestamp THEN
        RAISE EXCEPTION 'overlapping measurement interval (channel %, resolution %, timestamp %)',
            NEW.channel_id, NEW.resolution_seconds, NEW.timestamp
            USING ERRCODE = 'check_violation';
    END IF;
    RETURN NEW;
END $$;
