# RUNTIME KB BRANCHES AND ADMIN MCP

## Metadata

- **Status:** Settled
- **Owner Crates:** `quire-runtime`, `quire-session-protocol`
- **Related Notes:** `../notes/runtime_kb_branch_promotion.md`, `../notes/mcp_preview_tool_exposure.md`

## 1. Scope & Non-Goals

This document governs branch-scoped knowledge storage, immutable KB snapshots, Smart CAS promotion,
active rollback, session branch selection, and the runtime `/admin/mcp` control surface. It does not
cover report block Smart CAS in `quire-store`, report-authoring MCP tools in `quire-server`, or model
wire protocols beyond the provider registry already owned by `quire-runtime`.

## 2. Canonical Design & Contract

### Ownership

- `KbService` owns branch lifecycle, branch-scoped document/agent/skill CRUD, three-way merge,
  conflict reports, snapshot writes, rollback and branch-aware prompt caching.
- `ModelRegistry` owns provider/model persistence, models.dev import and masked provider receipts.
- `RuntimeService` owns session creation on a branch, session inspection and run orchestration.
- `http/` and `admin_mcp/` only deserialize inputs, rely on runtime Bearer authentication, call a
  service method and wrap the receipt. They contain no merge, persistence or run logic.

### Tables

The reset `0002_kb.sql` and `0004_runtime.sql` migrations define:

- `kb.branches(name PK, description, status, base_version, head_version, created_at, updated_at)`.
  Status is constrained to `active`, `draft`, `archived`; a partial unique index allows one active
  branch. Migration seeds `main` from the current `kb.meta.version`.
- `branch` on `kb.documents`, `kb.agent_definitions`, `kb.skill_definitions` and `kb.skill_resources`.
  Primary keys become `(branch, doc_id)`, `(branch, name)`, `(branch, name)` and
  `(branch, skill_name, path)`. Skill resources reference their branch-scoped skill definition.
- `kb.snapshots(version PK, branch, source, entries JSONB, created_at)`. Entries contain complete
  documents, agents and skills (skills include resources). Snapshots are immutable.
- `runtime.sessions.kb_branch`.

The same migration adds `kb.branch_conflicts(branch PK, active_version, branch_head_version,
created_at)`.

### Versioning and isolation

`kb.meta.version` is the global production version. Active mutations, promotions and rollbacks bump
it and publish `runtime:kb:invalidate`. Draft mutations only increment that branch's `head_version`;
they do not bump production, rewrite active rows, or publish a production release. Session prompt
caches are keyed by branch (`system_prompt:{branch}`), so an isolated branch session reads the draft
documents and skills while a normal session keeps reading active.

`CreateSessionRequest` accepts optional `kb_branch`; omission selects the active branch. New sessions
store the resolved branch name in `runtime.sessions.kb_branch` and expose it as
`SessionSummary.kb_branch`. Archived branches cannot back new sessions but remain readable for
already-created sessions.

### Promotion and conflicts

A draft stores `base_version` (the active head at creation) and `head_version`. Creating the branch
writes a `branch_base` snapshot at `base_version`. Promotion locks the active and draft rows and
compares base, active and draft entries per `(item_kind, item_key)` with content-only equality:

- Active unchanged since base: fast-forward, mode `fast_forward`.
- Only one side changed an item: take that side.
- Both sides changed it identically: take that side.
- Both changed it differently: reject with HTTP 409 / MCP invalid-params error containing
  `item_kind`, `item_key`, `base_content`, `active_content`, `branch_content`, plus branch and
  version metadata.

On rejection, `kb.branch_conflicts` records the draft head. If the draft is edited afterward, the
next promotion treats the new head as the resolution: it writes a `conflict_rebase` snapshot of the
current active state, moves the draft `base_version` to that version, merges and reports mode
`merged`. An unedited retry still returns the conflict.

Successful promotion applies only changed items to active, bumps the global version, archives the
draft, writes a `promote` snapshot, invalidates the local moka prompt cache and publishes Redis KB
invalidation. Rollback restores all active entries from a snapshot, bumps the global version, writes
a `rollback` snapshot and publishes the same invalidation. Omitting the rollback version restores the
latest snapshot before the current active head (one click); passing a version restores that specific
snapshot.

### REST routes

All routes use the existing runtime Bearer token:

- `GET /api/kb/branches`
- `POST /api/kb/branches` with `{name, description, from?}`
- `GET /api/kb/branches/{name}/diff`
- `POST /api/kb/branches/{name}/promote`
- `POST /api/kb/rollback` with optional `{version}`

Existing `/api/kb/docs`, `/api/kb/agents` and `/api/kb/skills` routes continue to target the active
branch.

### `/admin/mcp`

`admin_mcp/` mounts an rmcp Streamable HTTP server at `/admin/mcp`, protected by `runtime_auth`.
It is separate from the report-authoring `/mcp` surface and is not exposed to report sub-agents.
Every input uses `#[serde(deny_unknown_fields)]`; every receipt is compact and carries `_next_hint`.
Provider API keys are only returned through `ProviderRecord.api_key_masked`.

Tools:

- Branches: `kb_list_branches`, `kb_create_branch`, `kb_diff_branch`, `kb_promote_branch`,
  `kb_rollback_active`
- Branch items: `kb_list_items`, `kb_read_item`, `kb_upsert_doc`, `kb_upsert_agent`,
  `kb_upsert_skill`
- Models: `models_list_providers`, `models_upsert_provider`, `models_list`,
  `models_import_models_dev`
- Sessions: `sessions_list`, `sessions_inspect` (full `SessionDetail`, latest run status and
  `context_usage`)
- Diagnostics: `preview_report`, delegated to the same `PreviewToolService` used by native and
  report-authoring MCP calls

`RUNTIME_MCP_ALLOWED_HOSTS` controls the Streamable HTTP Host allowlist.

## 3. Invariants & Verification

- Draft edits never appear in active lists or active session prompts until promotion.
- Only one branch may be `active`; promotion archives the draft and never renames active.
- Every promotion and rollback writes the new immutable release snapshot. A promotion that rebases
  a resolved conflict additionally records a `conflict_rebase` baseline so one-click rollback can
  restore the active state that existed immediately before release.
- Every promotion and rollback publishes cache invalidation.
- A same-item conflict response includes all three contents and blocks promotion until the draft is
  edited and retried.
- `/admin/mcp` rejects missing or invalid runtime Bearer tokens.

Integration coverage in `crates/quire-runtime/tests/runtime_integration.rs`:

- `branch_edits_are_isolated_and_sessions_use_the_selected_branch`
- `disjoint_changes_auto_merge_on_promotion_and_invalidate_caches`
- `conflicting_promotion_returns_three_way_content_then_rollback_restores_previous_release`
- `admin_mcp_exposes_branch_model_and_session_tools_end_to_end`

Full gate:

```bash
cargo fmt --all -- --check
cargo clippy --workspace --all-targets -- -D warnings
cargo test --workspace
```
