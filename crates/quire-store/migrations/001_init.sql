-- Core report protocol persistence.
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
    rank VARCHAR(64) COLLATE "C" NOT NULL,
    type VARCHAR(24) NOT NULL,
    payload JSONB NOT NULL,
    style JSONB NOT NULL,
    source_ids TEXT[] NOT NULL DEFAULT '{}',
    updated_rev BIGINT NOT NULL,
    PRIMARY KEY (doc_id, block_id)
);

CREATE INDEX IF NOT EXISTS idx_report_blocks_order
    ON report_blocks (doc_id, rank);

CREATE TABLE IF NOT EXISTS report_sources (
    doc_id VARCHAR(64) NOT NULL REFERENCES report_documents(doc_id) ON DELETE CASCADE,
    source_id VARCHAR(64) NOT NULL,
    normalized_key TEXT NOT NULL,
    metadata JSONB NOT NULL,
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
    created_at TIMESTAMPTZ NOT NULL DEFAULT NOW(),
    PRIMARY KEY (doc_id, rev_num)
);

CREATE INDEX IF NOT EXISTS idx_report_revisions_time
    ON report_revisions (doc_id, rev_num DESC);

CREATE TABLE IF NOT EXISTS report_snapshots (
    doc_id VARCHAR(64) NOT NULL REFERENCES report_documents(doc_id) ON DELETE CASCADE,
    rev_num BIGINT NOT NULL,
    snapshot_zstd BYTEA NOT NULL,
    PRIMARY KEY (doc_id, rev_num)
);
