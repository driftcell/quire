-- Side-channel review comments and source evidence excerpts.
-- Comment threads intentionally avoid revision/CAS writes and keep block_id as a soft reference.

CREATE TABLE IF NOT EXISTS report_comment_threads (
    doc_id VARCHAR(64) NOT NULL REFERENCES report_documents(doc_id) ON DELETE CASCADE,
    thread_id VARCHAR(64) NOT NULL,
    block_id TEXT NOT NULL,
    section_id TEXT,
    anchor_snapshot TEXT,
    quote JSONB,
    status TEXT NOT NULL CHECK (status IN ('open', 'addressed', 'archived')),
    created_rev BIGINT NOT NULL,
    addressed_rev BIGINT,
    created_at TIMESTAMPTZ NOT NULL DEFAULT NOW(),
    updated_at TIMESTAMPTZ NOT NULL DEFAULT NOW(),
    PRIMARY KEY (doc_id, thread_id)
);

CREATE INDEX IF NOT EXISTS idx_report_comment_threads_status
    ON report_comment_threads (doc_id, status, updated_at DESC);

CREATE TABLE IF NOT EXISTS report_comment_messages (
    message_id VARCHAR(64) PRIMARY KEY,
    doc_id VARCHAR(64) NOT NULL,
    thread_id VARCHAR(64) NOT NULL,
    author_type TEXT NOT NULL CHECK (author_type IN ('human', 'agent')),
    author TEXT NOT NULL,
    body TEXT NOT NULL,
    created_at TIMESTAMPTZ NOT NULL DEFAULT NOW(),
    CONSTRAINT report_comment_messages_thread_fk
        FOREIGN KEY (doc_id, thread_id)
        REFERENCES report_comment_threads (doc_id, thread_id)
        ON DELETE CASCADE
);

CREATE INDEX IF NOT EXISTS idx_report_comment_messages_thread
    ON report_comment_messages (doc_id, thread_id, created_at ASC);

-- Excerpt duplicates the JSONB metadata field so reviewers can query evidence directly.
ALTER TABLE report_sources
    ADD COLUMN IF NOT EXISTS excerpt TEXT;
