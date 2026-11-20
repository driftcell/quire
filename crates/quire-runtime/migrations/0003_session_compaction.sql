ALTER TABLE agent_sessions
    ADD COLUMN IF NOT EXISTS compaction_summary TEXT,
    ADD COLUMN IF NOT EXISTS compacted_up_to_message_id TEXT,
    ADD COLUMN IF NOT EXISTS compacted_at TIMESTAMPTZ;

CREATE INDEX IF NOT EXISTS idx_agent_sessions_compacted_at
    ON agent_sessions (compacted_at)
    WHERE compacted_at IS NOT NULL;
