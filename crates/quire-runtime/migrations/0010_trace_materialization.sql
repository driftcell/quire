-- Materialized per-run traces and replay retention cursors.
ALTER TABLE agent_runs ADD COLUMN IF NOT EXISTS trace_summary JSONB;
ALTER TABLE agent_sessions ADD COLUMN IF NOT EXISTS min_available_seq BIGINT NOT NULL DEFAULT 0;
