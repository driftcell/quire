-- Style presets catalog: user-saved and public shared report style bundles.

CREATE TABLE IF NOT EXISTS catalog.style_presets (
    style_id TEXT PRIMARY KEY,
    name TEXT NOT NULL,
    description TEXT NOT NULL DEFAULT '',
    category_tag TEXT NOT NULL DEFAULT '自定义',
    is_public BOOLEAN NOT NULL DEFAULT FALSE,
    owner_user_id TEXT REFERENCES identity.users(user_id) ON DELETE SET NULL,
    owner_display_name TEXT NOT NULL DEFAULT '',
    base_preset_id TEXT NOT NULL DEFAULT 'quire-folio',
    bundle JSONB NOT NULL DEFAULT '{}'::jsonb,
    created_at TIMESTAMPTZ NOT NULL DEFAULT NOW(),
    updated_at TIMESTAMPTZ NOT NULL DEFAULT NOW()
);

CREATE INDEX IF NOT EXISTS idx_style_presets_public_updated
    ON catalog.style_presets (is_public, updated_at DESC);

CREATE INDEX IF NOT EXISTS idx_style_presets_owner_updated
    ON catalog.style_presets (owner_user_id, updated_at DESC)
    WHERE owner_user_id IS NOT NULL;
