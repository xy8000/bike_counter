-- Adds optional GPS coordinates to counting stations (WGS84 decimal degrees).
-- The columns are nullable because not every source station has coordinates;
-- stations without coordinates are "not provided" and can be patched later.
ALTER TABLE counting_stations ADD COLUMN latitude DOUBLE PRECISION;
ALTER TABLE counting_stations ADD COLUMN longitude DOUBLE PRECISION;
