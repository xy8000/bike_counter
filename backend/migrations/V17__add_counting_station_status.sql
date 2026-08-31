-- Lifecycle status of a counting station. A station becomes `inactive` when a
-- provider update stops including it in its station output (it is no longer
-- imported/refreshed, but the existing data and map flag stay); everything else
-- stays `active`.
ALTER TABLE counting_stations ADD COLUMN status text NOT NULL DEFAULT 'active';

ALTER TABLE counting_stations
    ADD CONSTRAINT counting_stations_status_check
    CHECK (status IN ('active', 'inactive'));
