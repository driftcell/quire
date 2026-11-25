-- Knowledge-base domain: branches, snapshots, documents, agent and skill definitions.
-- Fresh schema only: runtime data is namespaced under the `kb` schema.

CREATE SCHEMA IF NOT EXISTS kb;

CREATE TABLE IF NOT EXISTS kb.meta (
    singleton BOOLEAN PRIMARY KEY DEFAULT TRUE CHECK (singleton),
    version BIGINT NOT NULL DEFAULT 1
);

INSERT INTO kb.meta (singleton, version)
VALUES (TRUE, 1)
ON CONFLICT (singleton) DO NOTHING;

CREATE TABLE IF NOT EXISTS kb.branches (
    name TEXT PRIMARY KEY,
    description TEXT NOT NULL,
    status TEXT NOT NULL CHECK (status IN ('active', 'draft', 'archived')),
    base_version BIGINT NOT NULL,
    head_version BIGINT NOT NULL,
    created_at TIMESTAMPTZ NOT NULL DEFAULT NOW(),
    updated_at TIMESTAMPTZ NOT NULL DEFAULT NOW()
);

CREATE UNIQUE INDEX IF NOT EXISTS idx_kb_branches_single_active
    ON kb.branches (status) WHERE status = 'active';

INSERT INTO kb.branches (name, description, status, base_version, head_version)
SELECT 'main', 'Production knowledge base', 'active', version, version
FROM kb.meta
ON CONFLICT (name) DO NOTHING;

CREATE TABLE IF NOT EXISTS kb.snapshots (
    version BIGINT PRIMARY KEY,
    branch TEXT NOT NULL,
    source TEXT NOT NULL,
    entries JSONB NOT NULL,
    created_at TIMESTAMPTZ NOT NULL DEFAULT NOW()
);

CREATE TABLE IF NOT EXISTS kb.branch_conflicts (
    branch TEXT PRIMARY KEY REFERENCES kb.branches(name) ON DELETE CASCADE,
    active_version BIGINT NOT NULL,
    branch_head_version BIGINT NOT NULL,
    created_at TIMESTAMPTZ NOT NULL DEFAULT NOW()
);

CREATE TABLE IF NOT EXISTS kb.documents (
    branch TEXT NOT NULL DEFAULT 'main' REFERENCES kb.branches(name) ON DELETE CASCADE,
    doc_id TEXT NOT NULL,
    name TEXT NOT NULL,
    content TEXT NOT NULL,
    version BIGINT NOT NULL DEFAULT 1,
    updated_at TIMESTAMPTZ NOT NULL DEFAULT NOW(),
    PRIMARY KEY (branch, doc_id)
);

CREATE INDEX IF NOT EXISTS idx_kb_documents_branch
    ON kb.documents (branch, name);

CREATE TABLE IF NOT EXISTS kb.agent_definitions (
    branch TEXT NOT NULL DEFAULT 'main' REFERENCES kb.branches(name) ON DELETE CASCADE,
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
    ON kb.agent_definitions (branch, name);

CREATE UNIQUE INDEX IF NOT EXISTS idx_agent_definitions_default_per_branch
    ON kb.agent_definitions (branch)
    WHERE is_default = TRUE;

CREATE TABLE IF NOT EXISTS kb.skill_definitions (
    branch TEXT NOT NULL DEFAULT 'main' REFERENCES kb.branches(name) ON DELETE CASCADE,
    name TEXT NOT NULL,
    description TEXT NOT NULL,
    body TEXT NOT NULL,
    version BIGINT NOT NULL DEFAULT 1,
    enabled BOOLEAN NOT NULL DEFAULT TRUE,
    updated_at TIMESTAMPTZ NOT NULL DEFAULT NOW(),
    PRIMARY KEY (branch, name)
);

CREATE INDEX IF NOT EXISTS idx_skill_definitions_branch
    ON kb.skill_definitions (branch, name);

CREATE TABLE IF NOT EXISTS kb.skill_resources (
    branch TEXT NOT NULL DEFAULT 'main' REFERENCES kb.branches(name) ON DELETE CASCADE,
    skill_name TEXT NOT NULL,
    path TEXT NOT NULL,
    content TEXT NOT NULL,
    version BIGINT NOT NULL DEFAULT 1,
    PRIMARY KEY (branch, skill_name, path),
    CONSTRAINT skill_resources_skill_fkey
        FOREIGN KEY (branch, skill_name)
        REFERENCES kb.skill_definitions(branch, name) ON DELETE CASCADE
);

CREATE INDEX IF NOT EXISTS idx_skill_resources_branch
    ON kb.skill_resources (branch, skill_name);
