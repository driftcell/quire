-- Structured report-comment snapshots attached to user turns and queued messages.

ALTER TABLE agent_messages
    ADD COLUMN IF NOT EXISTS comment_refs JSONB NOT NULL DEFAULT '[]'::jsonb;

ALTER TABLE session_pending_queue
    ADD COLUMN IF NOT EXISTS comment_refs JSONB NOT NULL DEFAULT '[]'::jsonb;
