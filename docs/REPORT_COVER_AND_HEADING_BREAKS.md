# REPORT COVER AND HEADING PAGE BREAKS

## Metadata

- **Status:** Settled
- **Owner Crates:** `quire-protocol`, `quire-server`, `web`
- **Related Notes:** `../notes/cover_and_heading_page_breaks.md`

## 1. Scope & Non-Goals

This document governs two A4 layout contracts:

- `cover.custom_html` rendering in continuous versus paged/print mode, including mandatory XSS
  sanitization.
- Document-level automatic heading page breaks configured through `configure_page_chrome` and the
  `PageChromeDialog`.

It does not cover built-in cover variants (`classic`, `modern`, `minimal`, `split`) beyond preserving
their existing inline behavior, the block bin-packing algorithm, table row slicing, or theme CSS
sanitization.

## 2. Canonical Design & Contract

### Protocol

`quire-protocol` defines `HeadingPageBreak` with snake-case serialization:

```rust
pub enum HeadingPageBreak {
    None,     // "none"
    H1,       // "h1"
    H1AndH2,  // "h1_and_h2"
}
```

`PageChromeConfig` carries `#[serde(default)] pub heading_page_break: HeadingPageBreak`; omitted
input deserializes to `none`. The canonical schema is
`contracts/report-protocol.schema.json`, regenerated with `just schema`, and the frontend type is
generated with `web/` `bun run gen:types`.

### MCP and guide

`configure_page_chrome` accepts an optional `heading_page_break`. Omitting it preserves the current
value; supplying it merges only that field, exactly like the header/footer slots and first-page
visibility flags. `get_protocol_guide("citations_and_css")` documents the three values and the
custom-cover behavior for agents.

### Custom HTML cover sanitization

`ReportCover` passes agent-authored `custom_html` through `sanitizeCoverHtml` before assigning it to
`innerHTML`. The sanitizer uses the browser-native `DOMParser` and:

- removes `script`, `iframe`, `object` and `embed` elements;
- removes every attribute whose lowercase name starts with `on`;
- removes `href`, `src`, `xlink:href`, `action` and `formaction` values whose decoded protocol is
  `javascript:` or `vbscript:`, ignoring whitespace and control characters.

Template placeholders (`{{title}}`, `{{subtitle}}`, `{{date}}`, `{{signature}}`) are substituted
before sanitization.

### Dual-mode cover rendering

`ReportCover` receives `isPaged`:

- **Paged or print (`isPaged=true`):** a custom-HTML cover has no bottom margin or separator. When
  `ReportViewer` detects `cover.enabled && cover.custom_html`, it passes `coverFullPage` to the
  paginator and `ReportPage`.
- **Continuous (`isPaged=false`):** the custom cover keeps its natural height and gains a soft bottom
  separator (`border-b`, `pb-8`); the TOC and body follow directly.

`ReportPage` treats the first page as a full cover page when `page.includeCover && coverFullPage`:

- header and footer are not rendered, regardless of `hide_*_on_first_page` or template contents;
- the cover is placed in a `flex h-full flex-col justify-center` container so short custom HTML is
  vertically centered in the A4 page;
- the page exposes `data-full-page-cover="true"` for tests and diagnostics.

### Pagination

`PaginateOptions` adds `coverFullPage` and `headingPageBreak`. When `coverHeight > 0` and
`coverFullPage` is true, page 1 is emitted immediately with `includeCover: true`, no items and no
TOC; TOC and body always begin on page 2. Without a full-page cover, the cover still shares page 1
with the TOC when it fits.

While iterating blocks, a heading starts a new page when its own `style.page_break_before` is true
or when the document policy matches its level (`h1` for level 1, `h1_and_h2` for levels 1-2). The
break only fires when the current page already has body items or a TOC, so a heading at the top of a
fresh page never produces an empty page. A TOC-only page is preserved rather than discarded.

### User settings

`web/src/lib/pageChrome.ts` normalizes missing or invalid `heading_page_break` values to `none`,
merges the field through the same local override as header/footer settings, and persists it in
`quire.pageChrome.v1.<doc_id>`. `PageChromeDialog` exposes a single-select ToggleGroup with
`不强制`, `一级标题` and `一、二级标题`; page-chrome presets preserve the current heading-break
choice.

## 3. Invariants & Verification

- A custom-HTML cover page in paged/print mode contains only the cover, never the TOC, body blocks,
  header or footer; TOC/body start on page 2.
- Continuous custom covers keep natural height with a bottom separator and flow into the TOC.
- `HeadingPageBreak` defaults to `none`; all three values round-trip through serde and the generated
  JSON Schema.
- Automatic heading breaks never emit a blank page when the heading is already at a page top.
- Sanitization removes the required executable elements, inline handlers and script protocols, but
  keeps ordinary layout markup and placeholders.

```bash
cargo fmt --all -- --check
cargo clippy --workspace --all-targets -- -D warnings
cargo test --workspace
```

Frontend verification:

```bash
cd web
bun run test          # pagination.test.ts covers cover isolation, heading breaks and sanitization
bun run typecheck
bun run lint
bun run build
playwright-cli run-code --filename=web/e2e/cover-and-heading-breaks.js
```

The Playwright scenario asserts the continuous-versus-paged cover difference, hidden page-1
header/footer, TOC starting on page 2, browser-side sanitization, and H1 page-break switching via
`PageChromeDialog` without blank pages.
