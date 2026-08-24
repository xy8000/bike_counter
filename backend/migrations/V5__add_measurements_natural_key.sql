-- Natural key for measurements: one row per (channel_id, timestamp).
--
-- Before enforcing the constraint, collapse any duplicates that a partially
-- completed import may have left behind (keep the row with the smallest id).

DELETE FROM measurements a
USING measurements b
WHERE a.channel_id = b.channel_id
  AND a.timestamp = b.timestamp
  AND a.id > b.id;

-- The unique constraint doubles as the (channel_id, timestamp) index used by
-- channel-filtered pagination and by idempotent `ON CONFLICT` writes.
ALTER TABLE measurements
    ADD CONSTRAINT measurements_channel_id_timestamp_key
    UNIQUE (channel_id, timestamp);
