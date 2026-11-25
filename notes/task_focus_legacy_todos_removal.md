# REMOVE LEGACY TASKFOCUS TODO REPLAY COMPATIBILITY

## Metadata

- **Status:** Accepted
- **Date:** 2026-11-25
- **Promoted To:** `../../docs/RUNTIME_SESSION_EVENTS.md`

## 1. Context & Problem

The `describe_task` / `update_todos` split initially retained an optional `todos` field on
`TaskFocus` solely to project historical payloads. That left a second checklist write path in both
backend trace aggregation and the frontend reducer, despite the current contract requiring
`TodosUpdated` to be the only checklist mutation event. No historical payload compatibility is
required.

## 2. Rejected Options & Why

1. **Keep `TaskFocus.todos` for replay only**: rejected because it preserves two checklist mutation
   paths and makes strict event orthogonality depend on payload provenance.
2. **Remove the Rust field but retain frontend legacy merging**: rejected because old browser payloads
   would still mutate the checklist through `task_focus`, leaving behavior inconsistent across backend
   and frontend projections.

## 3. Chosen Approach & Trade-offs

- Remove `todos` from `ModelStreamEvent::TaskFocus` and `SessionEventKind::TaskFocus`, including all
  constructors, generated JSON Schema and TypeScript contracts.
- Make `apply_task_focus` and the frontend `task_focus` reducer preserve only the existing
  `RunTaskFocus.todos` state; only `TodosUpdated` replaces that checklist.
- Keep `RunTaskFocus.todos` as the current aggregate UI state shared by focus and checklist events.
- Historical `task_focus` payloads carrying `todos` receive no dedicated compatibility handling and
  cannot mutate the projected checklist through that field. Existing traces must derive checklist
  state from `todos_updated` events.
- Remove the historical tool-preview fallback that read `todos` from `describe_task` inputs. UI
  checklist reconstruction accepts only `update_todos` inputs.
