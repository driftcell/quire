-- Completed runs keep only the derived cold trace, not raw session events.
ALTER TABLE agent_runs ADD COLUMN IF NOT EXISTS trace_summary JSONB;
