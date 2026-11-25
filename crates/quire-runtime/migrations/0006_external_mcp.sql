-- External MCP harness API keys and session provenance.

CREATE TABLE IF NOT EXISTS identity.api_keys (
    key_id TEXT PRIMARY KEY,
    user_id TEXT NOT NULL REFERENCES identity.users(user_id) ON DELETE CASCADE,
    name TEXT NOT NULL,
    key_prefix TEXT NOT NULL,
    key_hash TEXT NOT NULL UNIQUE,
    last_used_at TIMESTAMPTZ,
    created_at TIMESTAMPTZ NOT NULL DEFAULT NOW()
);

CREATE INDEX IF NOT EXISTS idx_api_keys_user_id
    ON identity.api_keys (user_id, created_at DESC);

ALTER TABLE runtime.sessions
    ADD COLUMN IF NOT EXISTS created_via TEXT NOT NULL DEFAULT 'chat';

ALTER TABLE runtime.sessions
    ADD CONSTRAINT agent_sessions_created_via_check
        CHECK (created_via IN ('chat', 'mcp'));
