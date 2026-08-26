-- Cap provider messages per data source.
--
-- The core persists only the first 1000 provider messages per data source and
-- adds a single truncation warning when that limit is exceeded, so a data source
-- ends up with at most 1001 rows. This migration (1) trims any existing rows
-- that violate the 1000-message cap so current data is compliant, and (2) adds a
-- database trigger as defense in depth that never allows more than 1001 rows per
-- data source, regardless of the code path that inserts.

-- 1) One-time cleanup: keep only the newest 1000 provider messages per data
--    source (ordered by occurred_at, then id as a tie-breaker within the same
--    timestamp).
DELETE FROM data_source_provider_messages AS m
USING (
    SELECT id
    FROM (
        SELECT
            id,
            row_number() OVER (
                PARTITION BY data_source_id
                ORDER BY occurred_at DESC, id DESC
            ) AS rn
        FROM data_source_provider_messages
    ) AS ranked
    WHERE ranked.rn > 1000
) AS excess
WHERE m.id = excess.id;

-- 2) Trigger function: after inserting a message, delete any rows beyond the
--    newest 1001 for that data source (the 1000 persisted events plus the one
--    truncation warning the core may add).
CREATE OR REPLACE FUNCTION enforce_data_source_provider_message_cap()
RETURNS trigger AS $$
BEGIN
    DELETE FROM data_source_provider_messages
    WHERE data_source_id = NEW.data_source_id
      AND id IN (
          SELECT id
          FROM data_source_provider_messages
          WHERE data_source_id = NEW.data_source_id
          ORDER BY occurred_at DESC, id DESC
          OFFSET 1001
      );
    RETURN NEW;
END;
$$ LANGUAGE plpgsql;

CREATE TRIGGER trg_data_source_provider_message_cap
AFTER INSERT ON data_source_provider_messages
FOR EACH ROW EXECUTE FUNCTION enforce_data_source_provider_message_cap();
