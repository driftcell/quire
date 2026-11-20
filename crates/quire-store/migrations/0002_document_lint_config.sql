ALTER TABLE report_documents
    ADD COLUMN IF NOT EXISTS lint JSONB NOT NULL DEFAULT '{"ignore":[]}'::jsonb;
