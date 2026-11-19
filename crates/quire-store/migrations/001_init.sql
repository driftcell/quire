-- Core report protocol persistence (clean squashed schema).
-- Ordering uses PostgreSQL "C" collation so fractional ranks match Rust byte ordering.

CREATE TABLE IF NOT EXISTS report_documents (
    doc_id VARCHAR(64) PRIMARY KEY,
    session_id VARCHAR(64),
    idempotency_key VARCHAR(128) UNIQUE,
    current_rev BIGINT NOT NULL DEFAULT 0,
    cover JSONB NOT NULL,
    toc JSONB NOT NULL,
    theme JSONB NOT NULL,
    "references" JSONB NOT NULL,
    created_at TIMESTAMPTZ NOT NULL DEFAULT NOW(),
    updated_at TIMESTAMPTZ NOT NULL DEFAULT NOW()
);

CREATE INDEX IF NOT EXISTS idx_report_documents_session
    ON report_documents (session_id, created_at DESC);
CREATE INDEX IF NOT EXISTS idx_report_documents_updated
    ON report_documents (updated_at DESC);

CREATE TABLE IF NOT EXISTS report_blocks (
    doc_id VARCHAR(64) NOT NULL REFERENCES report_documents(doc_id) ON DELETE CASCADE,
    block_id VARCHAR(64) NOT NULL,
    rank TEXT COLLATE "C" NOT NULL,
    type VARCHAR(24) NOT NULL,
    payload JSONB NOT NULL,
    style JSONB NOT NULL,
    source_ids TEXT[] NOT NULL DEFAULT '{}',
    updated_rev BIGINT NOT NULL,
    PRIMARY KEY (doc_id, block_id),
    CONSTRAINT idx_report_blocks_order UNIQUE (doc_id, rank) DEFERRABLE INITIALLY DEFERRED
);

CREATE TABLE IF NOT EXISTS report_sources (
    doc_id VARCHAR(64) NOT NULL REFERENCES report_documents(doc_id) ON DELETE CASCADE,
    source_id VARCHAR(64) NOT NULL,
    normalized_key TEXT NOT NULL,
    metadata JSONB NOT NULL,
    excerpt TEXT,
    PRIMARY KEY (doc_id, source_id)
);

CREATE UNIQUE INDEX IF NOT EXISTS idx_report_sources_normalized
    ON report_sources (doc_id, normalized_key);

CREATE TABLE IF NOT EXISTS report_revisions (
    doc_id VARCHAR(64) NOT NULL REFERENCES report_documents(doc_id) ON DELETE CASCADE,
    rev_num BIGINT NOT NULL,
    actor TEXT NOT NULL,
    summary TEXT NOT NULL,
    forward_ops JSONB NOT NULL,
    inverse_ops JSONB NOT NULL,
    affected_block_ids TEXT[] NOT NULL DEFAULT '{}',
    created_at TIMESTAMPTZ NOT NULL DEFAULT NOW(),
    PRIMARY KEY (doc_id, rev_num)
);

CREATE TABLE IF NOT EXISTS report_snapshots (
    doc_id VARCHAR(64) NOT NULL REFERENCES report_documents(doc_id) ON DELETE CASCADE,
    rev_num BIGINT NOT NULL,
    snapshot_zstd BYTEA NOT NULL,
    PRIMARY KEY (doc_id, rev_num)
);

-- Side-channel review comments (never increment revision or participate in Smart CAS).
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
