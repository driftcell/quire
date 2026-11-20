ALTER TABLE agent_sessions
    ADD COLUMN IF NOT EXISTS compaction_started_at TIMESTAMPTZ;

CREATE INDEX IF NOT EXISTS idx_agent_sessions_compaction_started_at
    ON agent_sessions (compaction_started_at)
    WHERE compaction_started_at IS NOT NULL;
