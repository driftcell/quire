-- One top-level active run per session, enforced across processes and rolling deploys.
-- The in-memory session gate remains a fast path; this index is the correctness backstop.
CREATE UNIQUE INDEX IF NOT EXISTS idx_agent_runs_one_active_per_session
    ON agent_runs (session_id)
    WHERE parent_run_id IS NULL AND status IN ('running', 'awaiting_input');
