set dotenv-load := true
set shell := ["bash", "-cu"]

default: check

# Start local PostgreSQL + Redis.
up:
    docker compose up -d postgres redis

down:
    docker compose down

logs:
    docker compose logs -f postgres redis

# Apply SQLx migrations from the store crate.
migrate:
    sqlx migrate run --source crates/quire-store/migrations

# Regenerate the offline SQLx query cache after schema/query changes.
prepare:
    cargo sqlx prepare --workspace -- --all-targets

fmt:
    cargo fmt --all

fmt-check:
    cargo fmt --all --check

check:
    cargo check --workspace

# Regenerate the canonical JSON Schema contracts.
schema:
    cargo run -p quire-protocol --example export_schema
    cargo run -p quire-session-protocol --example export_session_schema

# Unit tests that do not need live PostgreSQL/Redis.
test-unit:
    cargo test -p quire-protocol
    cargo test -p quire-session-protocol
    cargo test -p quire-store --lib
    cargo test -p quire-server --lib
    cargo test -p quire-runtime --lib

# Full test suite; creates a disposable database next to DATABASE_URL.
test:
    #!/usr/bin/env bash
    set -euo pipefail
    TEST_DATABASE_URL="${DATABASE_URL%/*}/report_core_test"
    sqlx database drop -y --force --database-url "$TEST_DATABASE_URL" >/dev/null 2>&1 || true
    sqlx database create --database-url "$TEST_DATABASE_URL"
    sqlx migrate run --database-url "$TEST_DATABASE_URL" --source crates/quire-store/migrations
    TEST_DATABASE_URL="$TEST_DATABASE_URL" cargo test --workspace

test-protocol:
    cargo test -p quire-protocol

test-session-protocol:
    cargo test -p quire-session-protocol

test-runtime:
    cargo test -p quire-runtime

test-store:
    cargo test -p quire-store

clippy:
    cargo clippy --workspace --all-targets -- -D warnings

# Full verification gate used phase by phase.
verify: fmt-check clippy test

# Start the API/MCP server.
server:
    cargo run -p quire-server --bin quire-server

# Start the independent session/agent runtime.
runtime:
    cargo run -p quire-runtime --bin quire-runtime

# End-to-end multi-agent scenario (requires the full stack).
e2e:
    cargo test -p quire-server --test e2e_multi_agent_workflow -- --nocapture

# Quick infrastructure health check.
status:
    docker compose ps
    @curl -fsS "http://127.0.0.1:$${PORT:-8080}/health" || true
