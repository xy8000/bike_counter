-- Replaceable data-source logo. Like a counting station's image, a data source
-- can carry a provider-served logo stored as an asset (a metadata row here plus
-- the binary content in S3-compatible object storage). The data source OWNS the
-- link (logo_asset_id) plus the persisted provider hash (logo_sha256) used for
-- hash-based change detection. Deleting an asset unlinks the data source
-- (ON DELETE SET NULL) instead of cascading.
ALTER TABLE data_sources ADD COLUMN logo_asset_id UUID REFERENCES assets(id) ON DELETE SET NULL;
ALTER TABLE data_sources ADD COLUMN logo_sha256 TEXT;
