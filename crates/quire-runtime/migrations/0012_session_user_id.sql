ALTER TABLE agent_sessions
    ADD COLUMN IF NOT EXISTS user_id TEXT REFERENCES users(user_id) ON DELETE SET NULL;

CREATE INDEX IF NOT EXISTS idx_agent_sessions_user_updated
    ON agent_sessions (user_id, updated_at DESC);
