# DEFAULT STYLE AND STREAMING PREVIEW PARITY

## Metadata

- **Status:** Accepted
- **Date:** 2026-11-26
- **Promoted To:** `../docs/RUNTIME_STYLE_PRESETS.md`

## 1. Context & Problem

The style center stored its default preset only in browser localStorage, while report creation happens
in the runtime through native agent tools or external MCP. New reports therefore used the protocol
fallback instead of the selected default. Passing only a theme preset name would still omit the preset's
CSS tokens, custom CSS, cover, TOC and reference configuration. Separately, ephemeral report blocks used
a second renderer with hardcoded colors and typography, so streaming output diverged from committed
blocks under the same report theme.

## 2. Rejected Options & Why

- **Apply the browser default after observing a new document.** Rejected because it works only while a
  particular browser is open, races agent writes, adds an avoidable second revision and cannot cover
  external MCP creation consistently.
- **Pass only `theme_preset` to `create_report`.** Rejected because a name does not carry the full style
  bundle and leaves most visual settings unchanged.
- **Keep patching the ephemeral renderer's CSS independently.** Rejected because every new style token or
  block variant would need duplicate changes and visual parity would remain unenforceable.
- **Run custom preview CSS in a second global scope.** Rejected because report CSS is already scoped to
  `.report-root`; preview blocks must remain under the same root and inherit its tokens instead.

## 3. Chosen Approach & Trade-offs

Persist a user's default style ID in PostgreSQL and resolve the complete preset through the existing
server-to-runtime external harness bridge during report creation. Explicit preset selections resolve the
same complete bundle; system calls without a user retain the protocol fallback, and idempotent replay
never reapplies a style. The frontend persists default changes through the runtime API while retaining
local preferences as display state.

Streaming and committed blocks will share the same style-aware rendering primitives. Ephemeral previews
stay inside the report's existing `.report-root`, inheriting its theme tokens and scoped custom CSS. The
accepted cost is a small runtime migration and a wider internal bridge contract in exchange for one
creation path and enforceable visual parity.
