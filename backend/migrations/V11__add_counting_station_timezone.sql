-- Adds the IANA timezone each counting station reports its measurements in.
-- The "last day" summary is computed in this timezone (DST-aware), and a
-- provider may serve stations from several timezones. Defaults to UTC so
-- existing rows stay valid; the import refreshes it from the provider record.
ALTER TABLE counting_stations ADD COLUMN timezone TEXT NOT NULL DEFAULT 'UTC';
