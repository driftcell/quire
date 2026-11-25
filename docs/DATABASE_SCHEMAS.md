# POSTGRESQL DOMAIN SCHEMAS AND MIGRATION RUNNER

## Metadata

- **Status:** Settled
- **Owner Crates:** `quire-store`, `quire-runtime`, `quire-migrate`
- **Related Notes:** `../notes/database_schema_namespacing.md`

## 1. Scope & Non-Goals

This document governs logical PostgreSQL namespaces, migration ownership, schema-qualified SQL and
Rig memory connection routing. Schemas separate domains and privileges; they are not physical table
partitions and do not independently improve query performance. The reset contract targets fresh
databases and provides no legacy `public` views, table aliases, or automatic data migration.

## 2. Canonical Design & Contract

### 2.1 Domain namespaces

| Schema     | Tables and ownership                                                                                                                                    |
| ---------- | ------------------------------------------------------------------------------------------------------------------------------------------------------- |
| `report`   | `documents`, `blocks`, `sources`, `revisions`, `snapshots`, `comment_threads`, `comment_messages`, `deep_checks`; owned by `quire-store`                |
| `runtime`  | `sessions`, `attachments`, `runs`, `tool_calls`, `questions`, `pending_queue`, `chat_charts`; owned by `quire-runtime`                                  |
| `kb`       | `meta`, `branches`, `snapshots`, `branch_conflicts`, `documents`, `agent_definitions`, `skill_definitions`, `skill_resources`; owned by `quire-runtime` |
| `catalog`  | `llm_providers`, `llm_models`, `mcp_servers`, `mcp_tools`; owned by `quire-runtime`                                                                     |
| `identity` | `users`; owned by `quire-runtime`                                                                                                                       |
| `rig`      | `rig_memory_logs`, `rig_memory_log_chunks`, `rig_memory_files`, using the fixed upstream `rig-postgres` names                                           |

Application SQL uses explicit schema-qualified table names for every table, foreign key, index target
and schema migration statement. Index names themselves are unqualified because PostgreSQL creates an
index in its parent table's schema. The current migrations create no objects in `public`.

### 2.2 Migration pipelines

`quire-store/migrations/001_init.sql` is the complete fresh report schema. Its SQLx migrator creates
`report` and records history in `report._sqlx_migrations`.

`quire-runtime/migrations` is an ordered fresh chain:

1. `0001_identity.sql`
2. `0002_kb.sql`
3. `0003_catalog.sql`
4. `0004_runtime.sql`
5. `0005_rig.sql`

The runtime migrator creates `runtime` and records history in `runtime._quire_runtime_migrations`.
`runtime.sessions.user_id` references `identity.users(user_id)` with `ON DELETE SET NULL`; runtime and
report data remain logically linked by IDs without new cross-domain cascades.

`quire-migrate` is a standalone binary with no application-crate dependency. It runs both pipelines
without compiling application query macros, which makes an empty-database bootstrap possible:

```bash
just migrate
```

`just test` recreates the disposable database, runs `quire-migrate`, and sets both `DATABASE_URL` and
`TEST_DATABASE_URL` to that database for compilation and test execution. The server retains its
embedded auto-migration path for normal startup.

### 2.3 Rig memory connections

Application-owned Rig cleanup SQL uses `rig.rig_memory_*`. `RuntimeStore` also creates a dedicated
Rig pool whose connections execute `SET search_path TO rig`. `ConversationMemory` passes this pool to
upstream `PostgresSessionStore`, whose unqualified fixed table names therefore resolve in `rig`.

The Rig migration pins the upstream `rig-postgres 0.46.0` DDL. Opening the store finds all three tables
already present and does not require DDL privileges or a fork. The main application pool continues to
use explicit schema qualification rather than an ordered multi-schema `search_path`.

## 3. Invariants & Verification

- Fresh migration creates all six schemas and both schema-local migration ledgers without creating
  objects in `public`.
- Application behavior tests exercise CRUD, Smart CAS, constraints, sessions, registry operations and
  canonical memory against real PostgreSQL/Redis; no hardcoded schema/table inventory test is used.
- Rig integration coverage performs actual memory reads and writes through the dedicated pool.
- The full verification gate is:

```bash
cargo fmt --all -- --check
cargo clippy --workspace --all-targets -- -D warnings
just test
```

Use a freshly migrated database for SQLx macro-aware Clippy/check commands. `just prepare` must be run
after query changes when maintaining an offline SQLx cache.
