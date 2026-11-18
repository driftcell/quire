-- Git-style knowledge-base branches, immutable snapshots and session branch selection.
--
-- The active branch keeps production knowledge. Draft branches are copy-on-write scopes for
-- isolated editing and test sessions. Promotion performs item-level three-way merging against
-- the branch base snapshot before it replaces active entries.

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

ALTER TABLE kb_documents
    ADD COLUMN branch TEXT NOT NULL DEFAULT 'main',
    DROP CONSTRAINT kb_documents_pkey,
    ADD PRIMARY KEY (branch, doc_id),
    ADD CONSTRAINT kb_documents_branch_fkey
        FOREIGN KEY (branch) REFERENCES kb_branches(name) ON DELETE CASCADE;

ALTER TABLE agent_definitions
    ADD COLUMN branch TEXT NOT NULL DEFAULT 'main',
    DROP CONSTRAINT agent_definitions_pkey,
    ADD PRIMARY KEY (branch, name),
    ADD CONSTRAINT agent_definitions_branch_fkey
        FOREIGN KEY (branch) REFERENCES kb_branches(name) ON DELETE CASCADE;

-- Detach the old skill resource keys before the skill definition primary key changes.
ALTER TABLE skill_resources
    ADD COLUMN branch TEXT NOT NULL DEFAULT 'main',
    DROP CONSTRAINT skill_resources_pkey,
    DROP CONSTRAINT skill_resources_skill_name_fkey;

ALTER TABLE skill_definitions
    ADD COLUMN branch TEXT NOT NULL DEFAULT 'main',
    DROP CONSTRAINT skill_definitions_pkey,
    ADD PRIMARY KEY (branch, name),
    ADD CONSTRAINT skill_definitions_branch_fkey
        FOREIGN KEY (branch) REFERENCES kb_branches(name) ON DELETE CASCADE;

ALTER TABLE skill_resources
    ADD PRIMARY KEY (branch, skill_name, path),
    ADD CONSTRAINT skill_resources_branch_fkey
        FOREIGN KEY (branch) REFERENCES kb_branches(name) ON DELETE CASCADE,
    ADD CONSTRAINT skill_resources_skill_fkey
        FOREIGN KEY (branch, skill_name)
        REFERENCES skill_definitions(branch, name) ON DELETE CASCADE;

CREATE INDEX IF NOT EXISTS idx_kb_documents_branch ON kb_documents (branch, name);
CREATE INDEX IF NOT EXISTS idx_agent_definitions_branch ON agent_definitions (branch, name);
CREATE INDEX IF NOT EXISTS idx_skill_definitions_branch ON skill_definitions (branch, name);
CREATE INDEX IF NOT EXISTS idx_skill_resources_branch ON skill_resources (branch, skill_name);

ALTER TABLE agent_sessions ADD COLUMN kb_branch TEXT;

CREATE INDEX IF NOT EXISTS idx_agent_sessions_branch ON agent_sessions (kb_branch);
