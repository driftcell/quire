ALTER TABLE agent_sessions
    ADD COLUMN IF NOT EXISTS pinned_at TIMESTAMPTZ;

CREATE INDEX IF NOT EXISTS idx_agent_sessions_pinned_at
    ON agent_sessions (pinned_at DESC)
    WHERE pinned_at IS NOT NULL;
