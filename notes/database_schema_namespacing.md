# NAMESPACE POSTGRESQL TABLES BY DOMAIN SCHEMA

## Metadata

- **Status:** Accepted
- **Date:** 2026-11-25
- **Promoted To:** `../../docs/DATABASE_SCHEMAS.md`

## 1. Context & Problem

All Quire and Rig tables currently share `public`, with name prefixes acting as ad-hoc namespaces.
This obscures ownership, prevents schema-scoped privileges, and mixes two migration pipelines with
third-party canonical memory DDL. The migration history may be reset, and no compatibility views or
legacy `public` table names are required. PostgreSQL schemas provide logical namespacing only; this
change is not physical table partitioning and is not intended to improve query performance by itself.

## 2. Rejected Options & Why

1. **Keep prefixed tables in `public`**: rejected because ownership and privileges remain implicit and
   third-party tables share the same namespace as application tables.
2. **Create one schema per existing prefix**: rejected because mechanical splitting produces shallow
   namespaces rather than domain boundaries.
3. **Use a global ordered `search_path` for every query**: rejected because unqualified application SQL
   remains sensitive to connection state and name-resolution order.
4. **Fork or rename `rig-postgres` memory tables**: rejected because the locked upstream explicitly
   supports selecting its current schema through a dedicated connection pool.

## 3. Chosen Approach & Trade-offs

- Use six domain schemas: `report`, `runtime`, `kb`, `catalog`, `identity`, and `rig`.
- Keep two migration pipelines: `quire-store` owns `report`; `quire-runtime` owns ordered identity,
  KB, catalog, runtime, and pinned Rig initialization migrations. Migration ledgers live at
  `report._sqlx_migrations` and `runtime._quire_runtime_migrations` using SQLx schema support.
- Fully qualify all application-owned SQL. Remove redundant table prefixes where the schema already
  supplies the namespace, while retaining useful `llm_` / `mcp_` subdomain names and upstream Rig names.
- Give `ConversationMemory` a dedicated pool whose connections use `search_path=rig`; upstream
  `PostgresSessionStore` therefore resolves its unqualified fixed table names in `rig` without a fork.
- Do not create legacy views in `public` and do not add physical table partitioning. The accepted cost
  is a one-time migration/query rewrite and an additional Rig connection pool.
