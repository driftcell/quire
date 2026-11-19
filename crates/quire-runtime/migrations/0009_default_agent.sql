ALTER TABLE agent_definitions
    ADD COLUMN IF NOT EXISTS is_default BOOLEAN NOT NULL DEFAULT FALSE;

CREATE UNIQUE INDEX IF NOT EXISTS idx_agent_definitions_default_per_branch
    ON agent_definitions (branch)
    WHERE is_default = TRUE;
