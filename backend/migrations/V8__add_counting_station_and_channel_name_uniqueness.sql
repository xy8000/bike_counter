-- Unique counting-station (per data source) and channel (per counting station)
-- names.
--
-- The Münster archive repeats channel names within a station, so already
-- imported data may contain duplicates. Repair those rows first by appending
-- the external id (falling back to the row's UUID when no external id exists),
-- then enforce the invariants with unique indexes.

-- Channels: rename all but the first row of each (counting_station_id, name)
-- group by appending the external id.
UPDATE channels
SET name = name || ' (' || COALESCE(external_datasource_id, id::text) || ')'
WHERE id IN (
    SELECT id FROM (
        SELECT id,
               row_number() OVER (
                   PARTITION BY counting_station_id, name ORDER BY id
               ) AS rn
        FROM channels
    ) ranked
    WHERE rn > 1
);

-- Counting stations: same repair, scoped per data source (only rows that are
-- actually linked to a data source).
UPDATE counting_stations
SET name = name || ' (' || COALESCE(external_datasource_id, id::text) || ')'
WHERE id IN (
    SELECT id FROM (
        SELECT id,
               row_number() OVER (
                   PARTITION BY data_source_id, name ORDER BY id
               ) AS rn
        FROM counting_stations
        WHERE data_source_id IS NOT NULL
    ) ranked
    WHERE rn > 1
);

-- A counting-station name must be unique per data source. data_source_id is
-- nullable (unlinked stations), so the index is partial.
CREATE UNIQUE INDEX idx_counting_stations_data_source_id_name
    ON counting_stations (data_source_id, name)
    WHERE data_source_id IS NOT NULL;

-- A counting station must not have two channels with the same name.
CREATE UNIQUE INDEX idx_channels_counting_station_id_name
    ON channels (counting_station_id, name);
