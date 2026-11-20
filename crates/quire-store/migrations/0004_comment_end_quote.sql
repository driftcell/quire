ALTER TABLE report_comment_threads
    ADD COLUMN IF NOT EXISTS end_quote JSONB;
