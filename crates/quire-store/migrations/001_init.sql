-- Core report protocol persistence (single squashed schema).
-- Report-owned tables live in the dedicated `report` schema; all application SQL qualifies them.
-- Ordering uses PostgreSQL "C" collation so fractional ranks match Rust byte ordering.
--
-- `PgReportStore::migrate` also creates this schema through SQLx `Migrator::create_schema`, and
-- keeps its ledger at `report._sqlx_migrations`. The statement below keeps the file self-contained
-- for tooling that applies migrations directly (for example `sqlx migrate run`).

CREATE SCHEMA IF NOT EXISTS report;

CREATE TABLE IF NOT EXISTS report.documents (
    doc_id VARCHAR(64) PRIMARY KEY,
    session_id VARCHAR(64),
    idempotency_key VARCHAR(128) UNIQUE,
    current_rev BIGINT NOT NULL DEFAULT 0,
    cover JSONB NOT NULL,
    toc JSONB NOT NULL,
    theme JSONB NOT NULL,
    "references" JSONB NOT NULL,
    created_at TIMESTAMPTZ NOT NULL DEFAULT NOW(),
    updated_at TIMESTAMPTZ NOT NULL DEFAULT NOW(),
    lint JSONB NOT NULL DEFAULT '{"ignore":[]}'::jsonb,
    target_prose_chars BIGINT CHECK (target_prose_chars BETWEEN 1 AND 4294967295)
);

CREATE INDEX IF NOT EXISTS idx_report_documents_session
    ON report.documents (session_id, created_at DESC);
CREATE INDEX IF NOT EXISTS idx_report_documents_updated
    ON report.documents (updated_at DESC);

CREATE TABLE IF NOT EXISTS report.blocks (
    doc_id VARCHAR(64) NOT NULL REFERENCES report.documents(doc_id) ON DELETE CASCADE,
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

CREATE TABLE IF NOT EXISTS report.sources (
    doc_id VARCHAR(64) NOT NULL REFERENCES report.documents(doc_id) ON DELETE CASCADE,
    source_id VARCHAR(64) NOT NULL,
    normalized_key TEXT NOT NULL,
    metadata JSONB NOT NULL,
    excerpt TEXT,
    PRIMARY KEY (doc_id, source_id)
);

CREATE UNIQUE INDEX IF NOT EXISTS idx_report_sources_normalized
    ON report.sources (doc_id, normalized_key);

CREATE TABLE IF NOT EXISTS report.revisions (
    doc_id VARCHAR(64) NOT NULL REFERENCES report.documents(doc_id) ON DELETE CASCADE,
    rev_num BIGINT NOT NULL,
    actor TEXT NOT NULL,
    summary TEXT NOT NULL,
    forward_ops JSONB NOT NULL,
    inverse_ops JSONB NOT NULL,
    affected_block_ids TEXT[] NOT NULL DEFAULT '{}',
    created_at TIMESTAMPTZ NOT NULL DEFAULT NOW(),
    PRIMARY KEY (doc_id, rev_num)
);

CREATE TABLE IF NOT EXISTS report.snapshots (
    doc_id VARCHAR(64) NOT NULL REFERENCES report.documents(doc_id) ON DELETE CASCADE,
    rev_num BIGINT NOT NULL,
    snapshot_zstd BYTEA NOT NULL,
    PRIMARY KEY (doc_id, rev_num)
);

-- Side-channel review comments (never increment revision or participate in Smart CAS).
-- Columns are listed in the historical append order so fresh databases match migrated ones.
CREATE TABLE IF NOT EXISTS report.comment_threads (
    doc_id VARCHAR(64) NOT NULL REFERENCES report.documents(doc_id) ON DELETE CASCADE,
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
    end_block_id TEXT,
    anchor_blocks JSONB NOT NULL DEFAULT '[]'::jsonb,
    end_quote JSONB,
    evaluation_meta JSONB,
    PRIMARY KEY (doc_id, thread_id)
);

CREATE INDEX IF NOT EXISTS idx_report_comment_threads_status
    ON report.comment_threads (doc_id, status, updated_at DESC);
CREATE INDEX IF NOT EXISTS idx_report_comment_threads_anchor_blocks
    ON report.comment_threads USING GIN (anchor_blocks);

CREATE TABLE IF NOT EXISTS report.comment_messages (
    message_id VARCHAR(64) PRIMARY KEY,
    doc_id VARCHAR(64) NOT NULL,
    thread_id VARCHAR(64) NOT NULL,
    author_type TEXT NOT NULL CHECK (author_type IN ('human', 'agent')),
    author TEXT NOT NULL,
    body TEXT NOT NULL,
    created_at TIMESTAMPTZ NOT NULL DEFAULT NOW(),
    CONSTRAINT report_comment_messages_thread_fk
        FOREIGN KEY (doc_id, thread_id)
        REFERENCES report.comment_threads (doc_id, thread_id)
        ON DELETE CASCADE
);

CREATE INDEX IF NOT EXISTS idx_report_comment_messages_thread
    ON report.comment_messages (doc_id, thread_id, created_at ASC);

CREATE TABLE IF NOT EXISTS report.deep_checks (
    doc_id TEXT PRIMARY KEY REFERENCES report.documents(doc_id) ON DELETE CASCADE,
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
