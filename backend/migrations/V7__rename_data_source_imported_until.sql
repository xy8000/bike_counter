-- Rename the incremental import cursor to a clearer name: the watermark
-- timestamp up to which a data source's measurements have been imported.
ALTER TABLE data_sources RENAME COLUMN last_updated_at TO imported_until;
