Execute a lifecycle control action (`action`) on `session_id`:
- `"interrupt"`: Cancel the active run (requires an active running turn; optionally target a specific `run_id`).
- `"retry"`: Rewind conversation memory to the last user message and trigger a fresh run (requires at least one user message).
- `"compact"`: Summarize conversation history into a structured handoff and refresh the session's pinned system prompt to the latest version of its KB branch (requires the session to have no active running turn).
- `"delete"`: Permanently delete the session and its associated runs.
