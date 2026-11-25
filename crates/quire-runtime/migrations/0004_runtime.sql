-- Runtime domain: sessions, attachments, runs, tool calls, questions, queue and charts.
-- Fresh schema only: runtime data is namespaced under the `runtime` schema.

CREATE SCHEMA IF NOT EXISTS runtime;

CREATE TABLE IF NOT EXISTS runtime.sessions (
    session_id TEXT PRIMARY KEY,
    title TEXT NOT NULL,
    doc_id TEXT,
    linked_doc_id TEXT,
    kb_branch TEXT,
    status TEXT NOT NULL DEFAULT 'active',
    active_run_status TEXT NOT NULL DEFAULT 'idle',
    pinned_system_prompt TEXT,
    synced_kb_version BIGINT NOT NULL DEFAULT 0,
    context_usage JSONB NOT NULL DEFAULT '{}'::jsonb,
    created_at TIMESTAMPTZ NOT NULL DEFAULT NOW(),
    updated_at TIMESTAMPTZ NOT NULL DEFAULT NOW(),
    compacted_at TIMESTAMPTZ,
    compaction_started_at TIMESTAMPTZ,
    pinned_at TIMESTAMPTZ,
    user_id TEXT REFERENCES identity.users(user_id) ON DELETE SET NULL
);

CREATE INDEX IF NOT EXISTS idx_agent_sessions_branch
    ON runtime.sessions (kb_branch);

CREATE INDEX IF NOT EXISTS idx_agent_sessions_compacted_at
    ON runtime.sessions (compacted_at)
    WHERE compacted_at IS NOT NULL;

CREATE INDEX IF NOT EXISTS idx_agent_sessions_compaction_started_at
    ON runtime.sessions (compaction_started_at)
    WHERE compaction_started_at IS NOT NULL;

CREATE INDEX IF NOT EXISTS idx_agent_sessions_pinned_at
    ON runtime.sessions (pinned_at DESC)
    WHERE pinned_at IS NOT NULL;

CREATE INDEX IF NOT EXISTS idx_agent_sessions_user_updated
    ON runtime.sessions (user_id, updated_at DESC);

CREATE TABLE IF NOT EXISTS runtime.attachments (
    attachment_id TEXT PRIMARY KEY,
    session_id TEXT NOT NULL REFERENCES runtime.sessions(session_id) ON DELETE CASCADE,
    filename TEXT NOT NULL,
    mime_type TEXT NOT NULL,
    size_bytes BIGINT NOT NULL,
    storage_path TEXT NOT NULL,
    created_at TIMESTAMPTZ NOT NULL DEFAULT NOW(),
    storage_provider TEXT NOT NULL DEFAULT 'local',
    CONSTRAINT agent_attachments_storage_provider_check
        CHECK (storage_provider IN ('local', 'oss'))
);

CREATE TABLE IF NOT EXISTS runtime.runs (
    run_id TEXT PRIMARY KEY,
    session_id TEXT NOT NULL REFERENCES runtime.sessions(session_id) ON DELETE CASCADE,
    parent_run_id TEXT,
    agent_id TEXT NOT NULL,
    status TEXT NOT NULL,
    model TEXT,
    error TEXT,
    started_at TIMESTAMPTZ NOT NULL DEFAULT NOW(),
    finished_at TIMESTAMPTZ,
    start_seq BIGINT,
    trace_summary JSONB
);

CREATE INDEX IF NOT EXISTS idx_agent_runs_status
    ON runtime.runs (status, started_at);

-- One top-level active run per session, enforced across processes and rolling deploys.
-- The in-memory session gate remains a fast path; this index is the correctness backstop.
CREATE UNIQUE INDEX IF NOT EXISTS idx_agent_runs_one_active_per_session
    ON runtime.runs (session_id)
    WHERE parent_run_id IS NULL AND status IN ('running', 'awaiting_input');

CREATE TABLE IF NOT EXISTS runtime.tool_calls (
    tool_call_id TEXT PRIMARY KEY,
    session_id TEXT NOT NULL REFERENCES runtime.sessions(session_id) ON DELETE CASCADE,
    run_id TEXT NOT NULL,
    tool_name TEXT NOT NULL,
    status TEXT NOT NULL,
    input JSONB NOT NULL,
    output JSONB NOT NULL,
    error TEXT,
    duration_ms BIGINT NOT NULL DEFAULT 0,
    created_at TIMESTAMPTZ NOT NULL DEFAULT NOW()
);

CREATE TABLE IF NOT EXISTS runtime.questions (
    question_id TEXT PRIMARY KEY,
    session_id TEXT NOT NULL REFERENCES runtime.sessions(session_id) ON DELETE CASCADE,
    run_id TEXT NOT NULL,
    tool_call_id TEXT,
    questions JSONB NOT NULL,
    answers JSONB,
    status TEXT NOT NULL CHECK (status IN ('pending', 'answered', 'skipped', 'dismissed')),
    created_at TIMESTAMPTZ NOT NULL DEFAULT NOW(),
    resolved_at TIMESTAMPTZ
);

CREATE INDEX IF NOT EXISTS idx_agent_questions_pending
    ON runtime.questions (session_id, created_at DESC) WHERE status = 'pending';

CREATE INDEX IF NOT EXISTS idx_agent_questions_run
    ON runtime.questions (run_id);

-- Per-session FIFO queue for messages submitted while a run is active.
-- Queue rows survive runtime restarts and are drained one run at a time.
CREATE TABLE IF NOT EXISTS runtime.pending_queue (
    queue_id TEXT PRIMARY KEY,
    session_id TEXT NOT NULL REFERENCES runtime.sessions(session_id) ON DELETE CASCADE,
    client_msg_id TEXT NOT NULL,
    content TEXT NOT NULL,
    source TEXT NOT NULL CHECK (source IN ('user', 'comment')),
    thread_id TEXT,
    attachment_ids JSONB NOT NULL DEFAULT '[]'::jsonb,
    sequence BIGSERIAL NOT NULL,
    created_at TIMESTAMPTZ NOT NULL DEFAULT NOW(),
    comment_refs JSONB NOT NULL DEFAULT '[]'::jsonb,
    UNIQUE (session_id, client_msg_id)
);

CREATE INDEX IF NOT EXISTS idx_session_pending_queue_session
    ON runtime.pending_queue (session_id, sequence);

CREATE INDEX IF NOT EXISTS idx_session_pending_queue_thread
    ON runtime.pending_queue (thread_id) WHERE thread_id IS NOT NULL;

CREATE TABLE IF NOT EXISTS runtime.chat_charts (
    chart_id TEXT PRIMARY KEY,
    session_id TEXT NOT NULL REFERENCES runtime.sessions(session_id) ON DELETE CASCADE,
    payload JSONB NOT NULL,
    created_at TIMESTAMPTZ NOT NULL DEFAULT NOW(),
    updated_at TIMESTAMPTZ NOT NULL DEFAULT NOW()
);

CREATE INDEX IF NOT EXISTS idx_chat_charts_session
    ON runtime.chat_charts (session_id);
