# SPLIT LIVE WORK-STATUS BROADCAST (`describe_task`) AND DYNAMIC CHECKLIST MANAGEMENT (`update_todos`)

## Metadata

- **Status:** Accepted
- **Date:** 2026-11-25
- **Superseded In Part By:** `task_focus_legacy_todos_removal.md`
- **Promoted To:** `../../docs/RUNTIME_SESSION_EVENTS.md`, `../../docs/TOOL_DESCRIPTIONS.md`

## 1. Context & Problem

Previously, `describe_task` handled both live activity reporting (`title`, `detail`, `section_id`) and the structured stage checklist (`todos`), coupled through `merge_task_todos` and `SessionEventKind::TaskFocus`:

- Calling `describe_task(title="...")` without `todos` attempted to advance the existing checklist by matching `title` against existing todo items. Whenever the model described a concrete sub-action whose wording differed from a macro stage title (or on turn resume via `"正在从断点恢复任务"`), the projection prematurely marked the active macro todo `completed` and appended the status phrase as a synthetic todo item.
- Conversely, to avoid corrupting the checklist, prompts forced the lead agent to repeat coarse todo stage titles in `describe_task`, leaving the live header frozen on the macro stage name while sub-tasks progressed for tens of minutes, and `RunActivityHeader` suppressed `model.detail` whenever any todo list existed.

## 2. Rejected Options & Why

1. **Keep a single `describe_task` tool with a boolean mode flag (e.g. `update_todo_progress: bool`)**:
   Rejected because LLMs continue to conflate both responsibilities in one schema, creating ambiguous parallel tool batches when updating checklist steps and live micro-status concurrently under `tool_concurrency(8)`.
2. **Restrict `update_todos` exclusively to the lead agent**:
   Rejected because multi-step subagents (`researcher`, `analyst`, `writer`, `humanizer`, `reviewer`, `designer`) benefit from maintaining their own short 2–4 step execution checklists independently of broadcasting current sub-actions, and the user requested both capabilities across both lead and subagent roles.
3. **Emit `TaskFocus` from `update_todos` carrying the current `in_progress` todo title**:
   Rejected because concurrent batches (`describe_task` + `update_todos` in the same turn) would race on `TaskFocus.title`, letting a slower `update_todos` overwrite a specific live `describe_task` status string.

## 3. Chosen Approach & Trade-offs

- **Two orthogonal tools with explicit role boundaries**:
  - `describe_task(title, detail?, section_id?)` broadcasts only live current work focus via `SessionEventKind::TaskFocus`, never mutates existing todos, and returns `DescribeTaskReceipt` echoing `active_todo` and `todo_progress`.
  - `update_todos(todos)` atomically replaces the run's structured checklist via `SessionEventKind::TodosUpdated { todos }` without overwriting an explicit `describe_task` status title, and returns `UpdateTodosReceipt` with updated progress and `current_focus`.
- **Commutative wire & UI state**:
  - `SessionEventKind::TaskFocus` updates `title`/`detail`/`section_id` while preserving prior `todos`.
  - `SessionEventKind::TodosUpdated` updates `todos` while preserving any existing `title`/`detail`/`section_id` on `RunTaskFocus`.
  - `RunActivityHeader` and `traceSegments` prioritize explicit `describe_task` focus titles for the top status line, fall back to the active todo title on main runs only when `describe_task` has not yet been called, and only hide collapsed `model.detail` when it duplicates the active todo description.
- **Cost accepted**:
  - Adds one new `SessionEventKind` variant (`todos_updated`), requiring schema regeneration (`just schema` + `bun run --cwd web gen:types`), and replaces incremental title-matching todo mutation in `quire-session-protocol` and the frontend milestone projector.
