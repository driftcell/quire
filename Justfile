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

# Apply both report and runtime migrations without compiling application query macros.
migrate:
    cargo run -p quire-migrate

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
    DATABASE_URL="$TEST_DATABASE_URL" cargo run -p quire-migrate
    DATABASE_URL="$TEST_DATABASE_URL" TEST_DATABASE_URL="$TEST_DATABASE_URL" cargo test --workspace

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

# Start the unified Quire server (API, MCP, session runtime, and agent execution).
server:
    cargo run -p quire-runtime --bin server

# Quick infrastructure health check.
status:
    docker compose ps
    @curl -fsS "http://127.0.0.1:$${PORT:-8080}/health" || true
