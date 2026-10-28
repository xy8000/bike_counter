-- Station and channel names are display data, not identity (GitHub issue #47).
--
-- A counting station is identified by its `id` (UUID); the provider key is
-- `external_datasource_id`, which is globally unique since V2. Names are not
-- unique upstream: Hessen publishes the same short code for several sites and
-- Münster repeats channel names within a station. The unique name indexes added
-- in V8 therefore turned legitimate provider data into import failures
-- (`23505 idx_counting_stations_data_source_id_name`, V8 repair for Münster
-- channels), which is why they are dropped here. Name collisions must never
-- abort an import and must never be "fixed" by renaming rows.
--
-- The identity indexes are untouched:
--   idx_counting_stations_external_datasource_id
--   idx_channels_external_datasource_id
--   measurements (channel_id, timestamp, resolution_seconds)

-- 1. Remove the name-uniqueness indexes introduced in V8 and re-create them
--    **non-unique**: the lookups they served (stations/channels of one parent,
--    ordered by name) stay indexed, but duplicate names are now allowed.
--    (Idempotent so the repair below can be re-run safely.)
DROP INDEX IF EXISTS idx_counting_stations_data_source_id_name;
DROP INDEX IF EXISTS idx_channels_counting_station_id_name;

CREATE INDEX IF NOT EXISTS idx_counting_stations_data_source_id_name
    ON counting_stations (data_source_id, name)
    WHERE data_source_id IS NOT NULL;

CREATE INDEX IF NOT EXISTS idx_channels_counting_station_id_name
    ON channels (counting_station_id, name);

-- 2. One-time repair: strip the suffix the old import code generated
--    ("<name> (<external id>)" — see V8, the Hessen scraping adapter and
--    `DataImportService::unique_station_name`). Only the exact suffix built from
--    the row's *own* external id is removed, so genuine provider names are left
--    untouched. Rows repaired by V8 with a UUID suffix (no external id) and rows
--    with the rare " #<n>" fallback keep their name: both remain valid now that
--    names are no longer unique.
UPDATE counting_stations
SET name = left(name, length(name) - length(' (' || external_datasource_id || ')'))
WHERE external_datasource_id IS NOT NULL
  AND length(external_datasource_id) > 0
  AND right(name, length(' (' || external_datasource_id || ')')) =
      ' (' || external_datasource_id || ')';

UPDATE channels
SET name = left(name, length(name) - length(' (' || external_datasource_id || ')'))
WHERE external_datasource_id IS NOT NULL
  AND length(external_datasource_id) > 0
  AND right(name, length(' (' || external_datasource_id || ')')) =
      ' (' || external_datasource_id || ')';
