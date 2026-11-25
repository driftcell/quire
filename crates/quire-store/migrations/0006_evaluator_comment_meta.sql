ALTER TABLE report_comment_threads
    ADD COLUMN IF NOT EXISTS evaluation_meta JSONB;

CREATE TABLE IF NOT EXISTS report_deep_checks (
    doc_id TEXT PRIMARY KEY REFERENCES report_documents(doc_id) ON DELETE CASCADE,
    status TEXT NOT NULL CHECK (status IN ('idle', 'running', 'completed', 'failed')),
    provider TEXT,
    model TEXT,
    revision BIGINT,
    evaluated_blocks INT NOT NULL DEFAULT 0,
    created_threads INT NOT NULL DEFAULT 0,
    escalated_count INT NOT NULL DEFAULT 0,
    error_message TEXT,
    started_at TIMESTAMPTZ,
    finished_at TIMESTAMPTZ,
    updated_at TIMESTAMPTZ NOT NULL DEFAULT NOW()
);
