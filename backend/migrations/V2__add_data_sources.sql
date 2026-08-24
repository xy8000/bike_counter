CREATE TABLE data_sources (
    id UUID PRIMARY KEY,
    name TEXT NOT NULL UNIQUE,
    provider_type TEXT NOT NULL
);

ALTER TABLE counting_stations ADD COLUMN external_datasource_id TEXT;
ALTER TABLE counting_stations ADD COLUMN data_source_id UUID
    REFERENCES data_sources(id) ON DELETE SET NULL;

ALTER TABLE channels ADD COLUMN external_datasource_id TEXT;

CREATE UNIQUE INDEX idx_counting_stations_external_datasource_id
    ON counting_stations (external_datasource_id);
CREATE UNIQUE INDEX idx_channels_external_datasource_id
    ON channels (external_datasource_id);
