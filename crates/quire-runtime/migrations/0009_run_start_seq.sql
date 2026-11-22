-- Starting sequence in the session's Redis hot trace projection.
ALTER TABLE agent_runs ADD COLUMN IF NOT EXISTS start_seq BIGINT;
