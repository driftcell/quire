-- Canonical rig-memory storage. DDL is pinned to rig-postgres 0.46.0 so the
-- upstream store opens against existing tables and never needs DDL privileges.

CREATE SCHEMA IF NOT EXISTS rig;

CREATE TABLE IF NOT EXISTS rig.rig_memory_logs (
    name TEXT PRIMARY KEY,
    len BIGINT NOT NULL
);

CREATE TABLE IF NOT EXISTS rig.rig_memory_log_chunks (
    name TEXT NOT NULL,
    start BIGINT NOT NULL,
    bytes BYTEA NOT NULL,
    PRIMARY KEY (name, start)
);

CREATE TABLE IF NOT EXISTS rig.rig_memory_files (
    name TEXT PRIMARY KEY,
    bytes BYTEA NOT NULL
);
