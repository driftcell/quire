-- Per-user default report style. Built-in IDs are intentionally allowed without a foreign key.

ALTER TABLE identity.users
    ADD COLUMN IF NOT EXISTS default_style_id TEXT NOT NULL DEFAULT 'quire-folio';
