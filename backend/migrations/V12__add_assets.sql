-- Adds the assets table (PostgreSQL stores metadata only; the binary content
-- lives in S3-compatible object storage, e.g. MinIO) and links each counting
-- station to its image asset. The counting station OWNS the link
-- (image_asset_id) plus the persisted provider image hash (image_sha256) used
-- for hash-based change detection during import. Deleting an asset unlinks the
-- station (ON DELETE SET NULL) instead of cascading.
CREATE TABLE assets (
    id UUID PRIMARY KEY,
    object_key TEXT NOT NULL UNIQUE,
    content_type TEXT NOT NULL,
    byte_size BIGINT NOT NULL,
    sha256 TEXT NOT NULL,
    origin TEXT NOT NULL CHECK (origin IN ('builtin', 'provider')),
    created_at TIMESTAMPTZ NOT NULL DEFAULT now(),
    updated_at TIMESTAMPTZ NOT NULL DEFAULT now()
);

ALTER TABLE counting_stations ADD COLUMN image_asset_id UUID REFERENCES assets(id) ON DELETE SET NULL;
ALTER TABLE counting_stations ADD COLUMN image_sha256 TEXT;
