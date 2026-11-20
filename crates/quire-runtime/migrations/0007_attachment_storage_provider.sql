-- Attachment storage provenance. `storage_path` holds a local file path for
-- `local` rows and an OSS object key for `oss` rows.

ALTER TABLE agent_attachments
    ADD COLUMN IF NOT EXISTS storage_provider TEXT NOT NULL DEFAULT 'local';

ALTER TABLE agent_attachments
    DROP CONSTRAINT IF EXISTS agent_attachments_storage_provider_check;

ALTER TABLE agent_attachments
    ADD CONSTRAINT agent_attachments_storage_provider_check
    CHECK (storage_provider IN ('local', 'oss'));
