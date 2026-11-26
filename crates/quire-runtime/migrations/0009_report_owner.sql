-- Authoritative report owner on report.documents so ACL does not rely on
-- historical session doc_id / linked_doc_id links.

ALTER TABLE report.documents
    ADD COLUMN IF NOT EXISTS owner_user_id TEXT;

UPDATE report.documents d
SET owner_user_id = s.user_id
FROM runtime.sessions s
WHERE d.owner_user_id IS NULL
  AND d.session_id IS NOT NULL
  AND s.session_id = d.session_id
  AND s.user_id IS NOT NULL;

CREATE INDEX IF NOT EXISTS idx_report_documents_owner_user
    ON report.documents (owner_user_id)
    WHERE owner_user_id IS NOT NULL;
