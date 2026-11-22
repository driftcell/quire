-- Independent agent-runtime persistence (clean squashed schema).
-- Intentionally separate from quire-store so runtime state evolves independently.

-- ---------------------------------------------------------------------------
-- Knowledge base branches, documents, agents, skills & snapshots
-- ---------------------------------------------------------------------------

CREATE TABLE IF NOT EXISTS kb_meta (
    singleton BOOLEAN PRIMARY KEY DEFAULT TRUE CHECK (singleton),
    version BIGINT NOT NULL DEFAULT 1
);

INSERT INTO kb_meta (singleton, version)
VALUES (TRUE, 1)
ON CONFLICT (singleton) DO NOTHING;

CREATE TABLE IF NOT EXISTS kb_branches (
    name TEXT PRIMARY KEY,
    description TEXT NOT NULL,
    status TEXT NOT NULL CHECK (status IN ('active', 'draft', 'archived')),
    base_version BIGINT NOT NULL,
    head_version BIGINT NOT NULL,
    created_at TIMESTAMPTZ NOT NULL DEFAULT NOW(),
    updated_at TIMESTAMPTZ NOT NULL DEFAULT NOW()
);

CREATE UNIQUE INDEX IF NOT EXISTS idx_kb_branches_single_active
    ON kb_branches (status) WHERE status = 'active';

INSERT INTO kb_branches (name, description, status, base_version, head_version)
SELECT 'main', 'Production knowledge base', 'active', version, version
FROM kb_meta
ON CONFLICT (name) DO NOTHING;

CREATE TABLE IF NOT EXISTS kb_snapshots (
    version BIGINT PRIMARY KEY,
    branch TEXT NOT NULL,
    source TEXT NOT NULL,
    entries JSONB NOT NULL,
    created_at TIMESTAMPTZ NOT NULL DEFAULT NOW()
);

CREATE TABLE IF NOT EXISTS kb_branch_conflicts (
    branch TEXT PRIMARY KEY REFERENCES kb_branches(name) ON DELETE CASCADE,
    active_version BIGINT NOT NULL,
    branch_head_version BIGINT NOT NULL,
    created_at TIMESTAMPTZ NOT NULL DEFAULT NOW()
);

CREATE TABLE IF NOT EXISTS kb_documents (
    branch TEXT NOT NULL DEFAULT 'main' REFERENCES kb_branches(name) ON DELETE CASCADE,
    doc_id TEXT NOT NULL,
    name TEXT NOT NULL,
    content TEXT NOT NULL,
    version BIGINT NOT NULL DEFAULT 1,
    updated_at TIMESTAMPTZ NOT NULL DEFAULT NOW(),
    PRIMARY KEY (branch, doc_id)
);

CREATE INDEX IF NOT EXISTS idx_kb_documents_branch
    ON kb_documents (branch, name);

CREATE TABLE IF NOT EXISTS agent_definitions (
    branch TEXT NOT NULL DEFAULT 'main' REFERENCES kb_branches(name) ON DELETE CASCADE,
    name TEXT NOT NULL,
    description TEXT NOT NULL DEFAULT '',
    model TEXT NOT NULL,
    system_prompt TEXT NOT NULL,
    allowed_tools JSONB NOT NULL DEFAULT '[]'::jsonb,
    is_default BOOLEAN NOT NULL DEFAULT FALSE,
    version BIGINT NOT NULL DEFAULT 1,
    enabled BOOLEAN NOT NULL DEFAULT TRUE,
    updated_at TIMESTAMPTZ NOT NULL DEFAULT NOW(),
    PRIMARY KEY (branch, name)
);

CREATE INDEX IF NOT EXISTS idx_agent_definitions_branch
    ON agent_definitions (branch, name);

CREATE UNIQUE INDEX IF NOT EXISTS idx_agent_definitions_default_per_branch
    ON agent_definitions (branch)
    WHERE is_default = TRUE;

CREATE TABLE IF NOT EXISTS skill_definitions (
    branch TEXT NOT NULL DEFAULT 'main' REFERENCES kb_branches(name) ON DELETE CASCADE,
    name TEXT NOT NULL,
    description TEXT NOT NULL,
    body TEXT NOT NULL,
    version BIGINT NOT NULL DEFAULT 1,
    enabled BOOLEAN NOT NULL DEFAULT TRUE,
    updated_at TIMESTAMPTZ NOT NULL DEFAULT NOW(),
    PRIMARY KEY (branch, name)
);

CREATE INDEX IF NOT EXISTS idx_skill_definitions_branch
    ON skill_definitions (branch, name);

CREATE TABLE IF NOT EXISTS skill_resources (
    branch TEXT NOT NULL DEFAULT 'main' REFERENCES kb_branches(name) ON DELETE CASCADE,
    skill_name TEXT NOT NULL,
    path TEXT NOT NULL,
    content TEXT NOT NULL,
    version BIGINT NOT NULL DEFAULT 1,
    PRIMARY KEY (branch, skill_name, path),
    CONSTRAINT skill_resources_skill_fkey
        FOREIGN KEY (branch, skill_name)
        REFERENCES skill_definitions(branch, name) ON DELETE CASCADE
);

CREATE INDEX IF NOT EXISTS idx_skill_resources_branch
    ON skill_resources (branch, skill_name);

-- ---------------------------------------------------------------------------
-- Sessions, messages, runs, events, tool calls & questions
-- ---------------------------------------------------------------------------

CREATE TABLE IF NOT EXISTS agent_sessions (
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
    updated_at TIMESTAMPTZ NOT NULL DEFAULT NOW()
);

CREATE INDEX IF NOT EXISTS idx_agent_sessions_branch
    ON agent_sessions (kb_branch);

CREATE TABLE IF NOT EXISTS agent_attachments (
    attachment_id TEXT PRIMARY KEY,
    session_id TEXT NOT NULL REFERENCES agent_sessions(session_id) ON DELETE CASCADE,
    filename TEXT NOT NULL,
    mime_type TEXT NOT NULL,
    size_bytes BIGINT NOT NULL,
    storage_path TEXT NOT NULL,
    created_at TIMESTAMPTZ NOT NULL DEFAULT NOW()
);

CREATE TABLE IF NOT EXISTS agent_runs (
    run_id TEXT PRIMARY KEY,
    session_id TEXT NOT NULL REFERENCES agent_sessions(session_id) ON DELETE CASCADE,
    parent_run_id TEXT,
    agent_id TEXT NOT NULL,
    status TEXT NOT NULL,
    model TEXT,
    error TEXT,
    started_at TIMESTAMPTZ NOT NULL DEFAULT NOW(),
    finished_at TIMESTAMPTZ
);

CREATE INDEX IF NOT EXISTS idx_agent_runs_status
    ON agent_runs (status, started_at);

CREATE TABLE IF NOT EXISTS agent_tool_calls (
    tool_call_id TEXT PRIMARY KEY,
    session_id TEXT NOT NULL REFERENCES agent_sessions(session_id) ON DELETE CASCADE,
    run_id TEXT NOT NULL,
    tool_name TEXT NOT NULL,
    status TEXT NOT NULL,
    input JSONB NOT NULL,
    output JSONB NOT NULL,
    error TEXT,
    duration_ms BIGINT NOT NULL DEFAULT 0,
    created_at TIMESTAMPTZ NOT NULL DEFAULT NOW()
);

CREATE TABLE IF NOT EXISTS agent_questions (
    question_id TEXT PRIMARY KEY,
    session_id TEXT NOT NULL REFERENCES agent_sessions(session_id) ON DELETE CASCADE,
    run_id TEXT NOT NULL,
    tool_call_id TEXT,
    questions JSONB NOT NULL,
    answers JSONB,
    status TEXT NOT NULL CHECK (status IN ('pending', 'answered', 'skipped', 'dismissed')),
    created_at TIMESTAMPTZ NOT NULL DEFAULT NOW(),
    resolved_at TIMESTAMPTZ
);

CREATE INDEX IF NOT EXISTS idx_agent_questions_pending
    ON agent_questions (session_id, created_at DESC) WHERE status = 'pending';

CREATE INDEX IF NOT EXISTS idx_agent_questions_run
    ON agent_questions (run_id);

-- ---------------------------------------------------------------------------
-- LLM providers/models & MCP registry
-- ---------------------------------------------------------------------------

CREATE TABLE IF NOT EXISTS llm_providers (
    provider_id TEXT PRIMARY KEY,
    name TEXT NOT NULL,
    protocol TEXT NOT NULL CHECK (protocol IN ('openai_compatible', 'anthropic')),
    base_url TEXT NOT NULL,
    api_key_ref TEXT NOT NULL,
    enabled BOOLEAN NOT NULL DEFAULT TRUE,
    created_at TIMESTAMPTZ NOT NULL DEFAULT NOW(),
    updated_at TIMESTAMPTZ NOT NULL DEFAULT NOW()
);

CREATE TABLE IF NOT EXISTS llm_models (
    id TEXT PRIMARY KEY,
    provider_id TEXT NOT NULL REFERENCES llm_providers(provider_id) ON DELETE CASCADE,
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
    ON llm_models (provider_id, enabled);

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
