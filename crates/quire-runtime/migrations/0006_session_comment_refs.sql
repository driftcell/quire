-- Structured report-comment snapshots attached to queued messages.
-- Canonical conversation message metadata lives in rig-memory asides.

ALTER TABLE session_pending_queue
    ADD COLUMN IF NOT EXISTS comment_refs JSONB NOT NULL DEFAULT '[]'::jsonb;
