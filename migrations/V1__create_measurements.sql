CREATE TABLE measurements (
    id UUID PRIMARY KEY,
    value BIGINT NOT NULL,
    channel_id UUID NOT NULL,
    timestamp TIMESTAMPTZ NOT NULL
);