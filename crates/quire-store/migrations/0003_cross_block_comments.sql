ALTER TABLE report_comment_threads
    ADD COLUMN IF NOT EXISTS end_block_id TEXT;

ALTER TABLE report_comment_threads
    ADD COLUMN IF NOT EXISTS anchor_blocks JSONB NOT NULL DEFAULT '[]'::jsonb;

CREATE INDEX IF NOT EXISTS idx_report_comment_threads_anchor_blocks
    ON report_comment_threads USING GIN (anchor_blocks);
