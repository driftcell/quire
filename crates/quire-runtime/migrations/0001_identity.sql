-- Identity domain: local runtime accounts and credentials.
-- Fresh schema only: runtime data is namespaced under the `identity` schema.

CREATE SCHEMA IF NOT EXISTS identity;

CREATE TABLE IF NOT EXISTS identity.users (
    user_id TEXT PRIMARY KEY,
    username TEXT NOT NULL,
    email TEXT,
    display_name TEXT NOT NULL,
    role TEXT NOT NULL CHECK (role IN ('admin', 'user')),
    password_hash TEXT NOT NULL,
    enabled BOOLEAN NOT NULL DEFAULT TRUE,
    created_at TIMESTAMPTZ NOT NULL DEFAULT NOW(),
    updated_at TIMESTAMPTZ NOT NULL DEFAULT NOW()
);

CREATE UNIQUE INDEX IF NOT EXISTS idx_users_username_lower
    ON identity.users (LOWER(username));

CREATE UNIQUE INDEX IF NOT EXISTS idx_users_email_lower
    ON identity.users (LOWER(email))
    WHERE email IS NOT NULL;
