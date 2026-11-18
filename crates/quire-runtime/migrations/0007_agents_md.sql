-- Rename the legacy Claude Code prompt document to the generic AGENTS.md convention.
-- New workspaces seed AGENTS.md from agents/main.md; existing branches keep their content
-- but adopt the generic document key/name.
UPDATE kb_documents AS legacy
SET doc_id = 'agents-md', name = 'AGENTS.md'
WHERE legacy.doc_id = 'claude-md'
  AND NOT EXISTS (
      SELECT 1
      FROM kb_documents AS current
      WHERE current.branch = legacy.branch
        AND current.doc_id = 'agents-md'
  );
