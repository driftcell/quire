ALTER TABLE agent_definitions
    ADD COLUMN IF NOT EXISTS description TEXT NOT NULL DEFAULT '';
