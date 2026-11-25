CREATE TABLE IF NOT EXISTS chat_charts (
    chart_id TEXT PRIMARY KEY,
    session_id TEXT NOT NULL REFERENCES agent_sessions(session_id) ON DELETE CASCADE,
    payload JSONB NOT NULL,
    created_at TIMESTAMPTZ NOT NULL DEFAULT NOW(),
    updated_at TIMESTAMPTZ NOT NULL DEFAULT NOW()
);

CREATE INDEX IF NOT EXISTS idx_chat_charts_session ON chat_charts (session_id);
