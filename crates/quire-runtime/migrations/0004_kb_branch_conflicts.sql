-- Tracks the conflict report a draft branch received. A later branch edit is treated as an
-- explicit conflict resolution: promotion rebases the branch base onto the current active
-- snapshot and retries the merge.

CREATE TABLE IF NOT EXISTS kb_branch_conflicts (
    branch TEXT PRIMARY KEY REFERENCES kb_branches(name) ON DELETE CASCADE,
    active_version BIGINT NOT NULL,
    branch_head_version BIGINT NOT NULL,
    created_at TIMESTAMPTZ NOT NULL DEFAULT NOW()
);
