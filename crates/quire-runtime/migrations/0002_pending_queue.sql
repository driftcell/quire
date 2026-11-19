-- Per-session FIFO queue for messages submitted while a run is active.
-- Queue rows survive runtime restarts and are drained one run at a time.

CREATE TABLE IF NOT EXISTS session_pending_queue (
    queue_id TEXT PRIMARY KEY,
    session_id TEXT NOT NULL REFERENCES agent_sessions(session_id) ON DELETE CASCADE,
    client_msg_id TEXT NOT NULL,
    content TEXT NOT NULL,
    source TEXT NOT NULL CHECK (source IN ('user', 'comment')),
    thread_id TEXT,
    attachment_ids JSONB NOT NULL DEFAULT '[]'::jsonb,
    sequence BIGSERIAL NOT NULL,
    created_at TIMESTAMPTZ NOT NULL DEFAULT NOW(),
    UNIQUE (session_id, client_msg_id)
);

CREATE INDEX IF NOT EXISTS idx_session_pending_queue_session
    ON session_pending_queue (session_id, sequence);

CREATE INDEX IF NOT EXISTS idx_session_pending_queue_thread
    ON session_pending_queue (thread_id) WHERE thread_id IS NOT NULL;
