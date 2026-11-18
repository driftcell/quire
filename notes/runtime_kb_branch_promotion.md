# RUNTIME KB BRANCH PROMOTION MODEL

## Metadata

- **Status:** Accepted
- **Date:** 2026-11-18
- **Promoted To:** `../docs/RUNTIME_KB_BRANCHES_AND_ADMIN_MCP.md`

## 1. Context & Problem

`quire-runtime` evolved from a single mutable knowledge base into a workflow where an administrator
agent needs to edit documents, agents and skills in isolation, run real sessions against the draft,
then release with a Smart-CAS-style merge. The design had to answer four coupled questions:

- Whether promoting a branch changes the active branch *name* or only the active *content*.
- How to compare a draft against active without storing a second baseline copy of every item.
- How an administrator actually resolves a reported same-item conflict and retries.
- How `http.rs` and MCP can share behavior without duplicating merge or session logic.

## 2. Rejected Options & Why

- **Promote by switching the active branch name.** Rejected because `main` would become archived and
  default sessions would silently follow an arbitrary branch name. Content promotion keeps exactly
  one `active` row and makes production reads predictable.
- **Separate branch-only entry tables.** Rejected because every read, snapshot and seed path would
  need two query shapes. A composite `(branch, key)` primary key keeps one set of indexes and one
  CRUD implementation.
- **Per-item version vectors as merge metadata.** Rejected because the existing `version` columns are
  per-row counters, not causal clocks. A complete base snapshot is smaller to reason about, already
  required for rollback, and gives exact `base_content` in conflicts.
- **Treat any retry after a conflict as an implicit "accept branch".** Rejected because a no-op retry
  would overwrite active without an explicit edit. Persisting a conflict mark and requiring the draft
  head to advance makes the resolution deliberate.
- **Conflict resolution by mutating active directly.** Rejected as the default protocol: it violates
  branch isolation and bypasses the reviewable diff. Administrators may still edit active explicitly,
  but promotion remains the release operation.
- **A second MCP server with its own store access.** Rejected: merge, model and session observation
  behavior must remain in `KbService`, `ModelRegistry` and `RuntimeService` so REST and MCP cannot
  drift.

## 3. Chosen Approach & Trade-offs

`main` is the initial active branch. A draft is a copy-on-write scope sharing `(branch, key)` tables.
Creating a branch writes a `branch_base` snapshot at the source head version. Promotion compares that
base snapshot, current active entries and draft entries per `(item_kind, item_key)`:

- unchanged active: fast-forward;
- disjoint changes: automatic merge;
- same item changed differently: a 409/MCP error with `base_content`, `active_content` and
  `branch_content`.

A conflict writes `kb_branch_conflicts`. If the administrator edits the draft afterward (head
advances), the next promotion rebases the draft base onto the current active snapshot, making those
edits the resolution, then merges and archives the draft. Promotion and rollback bump the global
`kb_meta.version`, write immutable `kb_snapshots` rows and publish Redis KB invalidation.

The cost is that draft edits carry a local `head_version` counter rather than advancing the global
version, and an unedited retry after a conflict remains blocked by design.
