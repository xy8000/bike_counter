-- Heal current data and enforce that every counting station belongs to a data
-- source.
--
-- Rows imported before data-source linking existed have a NULL data_source_id.
-- The owning data source is not derivable from a row alone, so the backfill
-- only runs when exactly one data source is configured (true for the current
-- deployment). The DB then enforces the invariant with NOT NULL, so the core
-- never has to re-link stations: the adapter always provides the data_source_id
-- on insert.

UPDATE counting_stations
SET data_source_id = (SELECT id FROM data_sources LIMIT 1)
WHERE data_source_id IS NULL
  AND (SELECT count(*) FROM data_sources) = 1;

-- The original FK was ON DELETE SET NULL, which could orphan stations. With
-- NOT NULL, switch the whole chain to ON DELETE CASCADE so removing a data
-- source also removes its counting stations, channels and measurements.
ALTER TABLE counting_stations
    DROP CONSTRAINT IF EXISTS counting_stations_data_source_id_fkey;
ALTER TABLE counting_stations
    ALTER COLUMN data_source_id SET NOT NULL,
    ADD CONSTRAINT counting_stations_data_source_id_fkey
        FOREIGN KEY (data_source_id) REFERENCES data_sources(id) ON DELETE CASCADE;

ALTER TABLE channels
    DROP CONSTRAINT IF EXISTS channels_counting_station_id_fkey;
ALTER TABLE channels
    ADD CONSTRAINT channels_counting_station_id_fkey
        FOREIGN KEY (counting_station_id) REFERENCES counting_stations(id) ON DELETE CASCADE;

ALTER TABLE measurements
    DROP CONSTRAINT IF EXISTS measurements_channel_id_fkey;
ALTER TABLE measurements
    ADD CONSTRAINT measurements_channel_id_fkey
        FOREIGN KEY (channel_id) REFERENCES channels(id) ON DELETE CASCADE;
