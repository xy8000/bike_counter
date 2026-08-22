CREATE TABLE counting_stations (
    id UUID PRIMARY KEY,
    name TEXT NOT NULL,
    description TEXT NOT NULL
);

CREATE TABLE channels (
    id UUID PRIMARY KEY,
    counting_station_id UUID NOT NULL REFERENCES counting_stations(id),
    name TEXT NOT NULL,
    description TEXT NOT NULL
);

CREATE TABLE measurements (
    id UUID PRIMARY KEY,
    value BIGINT NOT NULL,
    channel_id UUID NOT NULL REFERENCES channels(id),
    timestamp TIMESTAMPTZ NOT NULL
);