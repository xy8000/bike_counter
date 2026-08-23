-- Opaque persistent key-value storage for provider state, scoped per data source.
-- A data source has exactly one provider, so data_source_id fully scopes the
-- state; there is no provider_type column. id is a random application-generated
-- UUID (Uuid::new_v4(), mirroring the jobs table) used purely as a stable record
-- identifier — no business meaning.
CREATE TABLE data_source_persistent_state (
    id UUID PRIMARY KEY,
    data_source_id UUID NOT NULL
        REFERENCES data_sources(id) ON DELETE CASCADE,
    key TEXT NOT NULL,
    value TEXT NOT NULL,
    updated_at TIMESTAMPTZ NOT NULL DEFAULT now(),
    CONSTRAINT uq_data_source_persistent_state_key
        UNIQUE (data_source_id, key)
);

-- FK lookups and cascade cleanup.
CREATE INDEX idx_data_source_persistent_state_data_source_id
    ON data_source_persistent_state (data_source_id);

-- Revoke persistent state at the DB level when a data source changes provider,
-- so a different provider never inherits the previous provider's memory.
CREATE OR REPLACE FUNCTION revoke_persistent_state_on_provider_change()
RETURNS TRIGGER AS $$
BEGIN
    IF OLD.provider_type IS DISTINCT FROM NEW.provider_type THEN
        DELETE FROM data_source_persistent_state WHERE data_source_id = NEW.id;
    END IF;
    RETURN NEW;
END;
$$ LANGUAGE plpgsql;

CREATE TRIGGER trg_revoke_persistent_state_on_provider_change
AFTER UPDATE OF provider_type ON data_sources
FOR EACH ROW
EXECUTE FUNCTION revoke_persistent_state_on_provider_change();
