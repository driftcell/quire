-- Pending ask_user questionnaire cards and their structured answers.

CREATE TABLE IF NOT EXISTS agent_questions (
    question_id TEXT PRIMARY KEY,
    session_id TEXT NOT NULL REFERENCES agent_sessions(session_id) ON DELETE CASCADE,
    run_id TEXT NOT NULL,
    tool_call_id TEXT,
    questions JSONB NOT NULL,
    answers JSONB,
    status TEXT NOT NULL CHECK (status IN ('pending', 'answered', 'skipped', 'dismissed')),
    created_at TIMESTAMPTZ NOT NULL DEFAULT NOW(),
    resolved_at TIMESTAMPTZ
);

CREATE INDEX IF NOT EXISTS idx_agent_questions_pending
    ON agent_questions (session_id, created_at DESC) WHERE status = 'pending';

CREATE INDEX IF NOT EXISTS idx_agent_questions_run
    ON agent_questions (run_id);
