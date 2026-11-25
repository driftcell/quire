-- Catalog domain: LLM providers/models and the global MCP server/tool registry.
-- Fresh schema only: runtime data is namespaced under the `catalog` schema.

CREATE SCHEMA IF NOT EXISTS catalog;

CREATE TABLE IF NOT EXISTS catalog.llm_providers (
    provider_id TEXT PRIMARY KEY,
    name TEXT NOT NULL,
    protocol TEXT NOT NULL CHECK (protocol IN ('openai_compatible', 'anthropic')),
    base_url TEXT NOT NULL,
    api_key_ref TEXT NOT NULL,
    enabled BOOLEAN NOT NULL DEFAULT TRUE,
    created_at TIMESTAMPTZ NOT NULL DEFAULT NOW(),
    updated_at TIMESTAMPTZ NOT NULL DEFAULT NOW()
);

CREATE TABLE IF NOT EXISTS catalog.llm_models (
    id TEXT PRIMARY KEY,
    provider_id TEXT NOT NULL REFERENCES catalog.llm_providers(provider_id) ON DELETE CASCADE,
    model_id TEXT NOT NULL,
    display_name TEXT NOT NULL,
    context_limit BIGINT NOT NULL,
    max_output_tokens BIGINT,
    supports_reasoning BOOLEAN NOT NULL DEFAULT FALSE,
    supports_tools BOOLEAN NOT NULL DEFAULT TRUE,
    supports_vision BOOLEAN NOT NULL DEFAULT FALSE,
    cost_input DOUBLE PRECISION,
    cost_output DOUBLE PRECISION,
    cost_cache_read DOUBLE PRECISION,
    enabled BOOLEAN NOT NULL DEFAULT TRUE,
    updated_at TIMESTAMPTZ NOT NULL DEFAULT NOW()
);

CREATE INDEX IF NOT EXISTS idx_llm_models_provider
    ON catalog.llm_models (provider_id, enabled);

CREATE TABLE IF NOT EXISTS catalog.mcp_servers (
    server_id TEXT PRIMARY KEY,
    name TEXT NOT NULL,
    url TEXT NOT NULL,
    auth_token_ref TEXT,
    enabled BOOLEAN NOT NULL DEFAULT TRUE,
    created_at TIMESTAMPTZ NOT NULL DEFAULT NOW(),
    updated_at TIMESTAMPTZ NOT NULL DEFAULT NOW()
);

CREATE TABLE IF NOT EXISTS catalog.mcp_tools (
    server_id TEXT NOT NULL REFERENCES catalog.mcp_servers(server_id) ON DELETE CASCADE,
    tool_name TEXT NOT NULL,
    description TEXT NOT NULL DEFAULT '',
    input_schema JSONB NOT NULL DEFAULT '{}'::jsonb,
    enabled BOOLEAN NOT NULL DEFAULT TRUE,
    updated_at TIMESTAMPTZ NOT NULL DEFAULT NOW(),
    PRIMARY KEY (server_id, tool_name)
);

CREATE INDEX IF NOT EXISTS idx_mcp_tools_enabled
    ON catalog.mcp_tools (server_id, tool_name) WHERE enabled;
