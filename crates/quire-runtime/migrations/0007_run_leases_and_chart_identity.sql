-- Recoverable run leases and session-scoped chat chart identity.

ALTER TABLE runtime.runs
    ADD COLUMN IF NOT EXISTS lease_owner TEXT,
    ADD COLUMN IF NOT EXISTS lease_expires_at TIMESTAMPTZ;

ALTER TABLE runtime.chat_charts
    DROP CONSTRAINT IF EXISTS chat_charts_pkey;

ALTER TABLE runtime.chat_charts
    ADD CONSTRAINT chat_charts_pkey PRIMARY KEY (session_id, chart_id);
