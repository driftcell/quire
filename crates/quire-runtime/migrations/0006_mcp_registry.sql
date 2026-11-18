-- Global MCP server registry and per-tool enable switches.
--
-- Auth token refs reuse the runtime's env:/enc: secret-reference mechanism. Raw tokens are never
-- stored; encrypted refs are only exposed through masked records.

CREATE TABLE IF NOT EXISTS mcp_servers (
    server_id TEXT PRIMARY KEY,
    name TEXT NOT NULL,
    url TEXT NOT NULL,
    auth_token_ref TEXT,
    enabled BOOLEAN NOT NULL DEFAULT TRUE,
    created_at TIMESTAMPTZ NOT NULL DEFAULT NOW(),
    updated_at TIMESTAMPTZ NOT NULL DEFAULT NOW()
);

CREATE TABLE IF NOT EXISTS mcp_tools (
    server_id TEXT NOT NULL REFERENCES mcp_servers(server_id) ON DELETE CASCADE,
    tool_name TEXT NOT NULL,
    description TEXT NOT NULL DEFAULT '',
    input_schema JSONB NOT NULL DEFAULT '{}'::jsonb,
    enabled BOOLEAN NOT NULL DEFAULT TRUE,
    updated_at TIMESTAMPTZ NOT NULL DEFAULT NOW(),
    PRIMARY KEY (server_id, tool_name)
);

CREATE INDEX IF NOT EXISTS idx_mcp_tools_enabled
    ON mcp_tools (server_id, tool_name) WHERE enabled;
