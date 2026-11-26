-- Indexes for session doc_id / linked_doc_id lookups and chart_id preview queries.

CREATE INDEX IF NOT EXISTS idx_agent_sessions_doc_id
    ON runtime.sessions (doc_id)
    WHERE doc_id IS NOT NULL;

CREATE INDEX IF NOT EXISTS idx_agent_sessions_linked_doc_id
    ON runtime.sessions (linked_doc_id)
    WHERE linked_doc_id IS NOT NULL;

CREATE INDEX IF NOT EXISTS idx_runtime_chat_charts_chart_updated
    ON runtime.chat_charts (chart_id, updated_at DESC);
