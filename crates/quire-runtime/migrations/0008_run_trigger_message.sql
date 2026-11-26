-- Track which user message triggered a run so idempotent retries can recover
-- if message persistence succeeded before run creation.

ALTER TABLE runtime.runs
    ADD COLUMN IF NOT EXISTS trigger_message_id TEXT;

CREATE INDEX IF NOT EXISTS idx_runtime_runs_trigger_message
    ON runtime.runs (session_id, trigger_message_id)
    WHERE trigger_message_id IS NOT NULL;
