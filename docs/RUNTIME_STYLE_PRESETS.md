# RUNTIME STYLE PRESETS AND PREVIEW PARITY

## Metadata

- **Status:** Settled
- **Owner Crates:** `quire-runtime`, `quire-server`
- **Related Notes:** `../notes/default_style_and_preview_parity.md`

## 1. Scope & Non-Goals

This document governs per-user default report styles, complete preset resolution during report creation,
and theme parity between ephemeral streaming blocks and committed report blocks. It does not define the
visual design of individual presets, chart preview lifecycle, or the public Block AST.

## 2. Canonical Design & Contract

### 2.1 Presets and user defaults

The runtime exposes ten built-in style presets and stores custom presets in `catalog.style_presets`.
`identity.users.default_style_id`, added by `0012_user_default_style.sql`, stores the user's selected
default. Built-in IDs are valid without a row in the custom preset table.

`PUT /api/styles/default/{style_id}` persists the authenticated user's default and returns 204. Built-in
and public presets are available to every user; private presets require ownership or administrator
access. Missing or inaccessible stored defaults resolve to `quire-folio`.

The browser still keeps `defaultStylePresetId` in UI preferences for immediate display. Setting a default
in the style center or applying a preset from the report dialog also persists it through the API. Starting
a conversation synchronizes the existing browser preference first, which migrates older local-only
choices without requiring another click.

### 2.2 Creation-time resolution

`quire_server::state::ExternalHarnessBridge::resolve_report_template` resolves either an explicitly
requested preset or the authenticated user's default. The runtime returns a complete
`ExternalReportTemplate`: `ThemeConfig`, `CoverConfig`, `TocConfig` and `ReferencesConfig`.

`QuireReportService::exec_create_report` applies this template before the initial document commit.
Request-specific title, cover metadata, explicit cover/TOC switches and an explicit preset name retain
their existing override behavior. Default application therefore does not create a second revision and
idempotent create replay cannot overwrite an existing report. Calls without a user identity retain the
protocol fallback. This path covers both native agent tools and external `/mcp` report creation.

### 2.3 Streaming visual parity

`ReportViewer` owns the report's `.report-root`, theme tokens, variant attributes and scoped custom CSS.
Ephemeral blocks remain under that same root. `ReportBlockPrimitives.tsx` supplies the shared heading,
paragraph, callout, quote and figure-caption rendering used by both `ReportBlockItem` and
`EphemeralPreviewCard`; streaming tables continue to use `TableBlockView`.

Preview-only skeletons and cursors use `--rpt-*` tokens rather than hardcoded palette classes. Committing
a block replaces only its streaming wrapper and cursor, not its typography or theme.

## 3. Invariants & Verification

- A new report created for a user with a default preset contains the complete preset at revision 1.
- Explicit preset resolution and default resolution use the same complete configuration path.
- Private presets remain owner/admin-only, and invalid stored defaults fall back safely.
- System calls without a user do not acquire an implicit user preference.
- Streaming and committed text blocks share the same style-aware primitives and report CSS scope.
- The runtime integration test `user_api_key_authenticates_report_mcp_returns_markdown_and_auto_binds_empty_session`
  sets `cyber-terminal` through REST and verifies its tokens, cover, TOC and references after MCP creation.
- Frontend tests assert shared heading/paragraph/quote markers, theme variables and the absence of
  hardcoded Slate text/background classes in previews.

Full gate:

```bash
cargo fmt --all -- --check
cargo clippy --workspace --all-targets -- -D warnings
just test
bun run --cwd web lint
bun run --cwd web typecheck
bun run --cwd web format:check
bun test --cwd web
bun run --cwd web build
```
