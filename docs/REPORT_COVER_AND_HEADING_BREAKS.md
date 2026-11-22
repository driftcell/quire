# REPORT COVER AND HEADING PAGE BREAKS

## Metadata

- **Status:** Settled
- **Owner Crates:** `quire-protocol`, `quire-server`, `web`
- **Related Notes:** `../notes/cover_and_heading_page_breaks.md`, `../notes/toc_pagination.md`

## 1. Scope & Non-Goals

This document governs three A4 layout contracts:

- `cover.custom_html` rendering in continuous versus paged/print mode, including mandatory XSS
  sanitization.
- Document-level automatic heading page breaks configured through `configure_page_chrome` and the
  `PageChromeDialog`.
- Document-level TOC page breaks configured through `configure_toc` and `TocConfig`.

It also records the built-in subtitle cleanup, TOC page-number alignment and measured table-slicing
invariants that protect A4 output. It does not cover theme CSS sanitization. The default centered cover
deliberately ends with natural `<header>` whitespace and no built-in bottom rule; agents that need a
rule or a special layout should provide `cover.custom_html`.

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

`TocConfig` carries `#[serde(default)] pub page_break_before: bool` and
`#[serde(default)] pub page_break_after: bool`. Both default to `false` and are first-class document
fields, not local display preferences. `TocConfig.variant` defaults to `dotted`; frontend normalization
uses the same default in continuous and paged modes when no explicit variant exists.

### MCP and guide

`configure_page_chrome` accepts an optional `heading_page_break`. Omitting it preserves the current
value; supplying it merges only that field, exactly like the header/footer slots and first-page
visibility flags. `get_protocol_guide("citations_and_css")` documents the three values and the
custom-cover behavior for agents.

`configure_toc` accepts optional `page_break_before` and `page_break_after`. Omitting either field
preserves the current value; supplying one merges only that field into `document.toc`.

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
- **Continuous (`isPaged=false`):** the custom cover keeps its natural height inside a full-bleed
  `continuous-custom-cover` wrapper. The wrapper owns negative top spacing, bottom spacing and
  `overflow-hidden`; `ReportCover` itself adds no border or bottom margin. The TOC and body follow
  directly.

`ReportPage` treats the first page as a full cover page when `page.includeCover && coverFullPage`:

- header and footer are not rendered, regardless of `hide_*_on_first_page` or template contents;
- the cover is placed in a `flex h-full flex-col justify-center` container so short custom HTML is
  vertically centered in the A4 page;
- the page exposes `data-full-page-cover="true"` for tests and diagnostics.

### Built-in cover and TOC details

Before any built-in variant or custom `{{subtitle}}` placeholder renders a subtitle, `ReportCover`
strips leading/trailing whitespace and dash characters. The `classic` variant then adds exactly one
`—— … ——` wrapper, so agent-supplied dashes cannot produce a doubled prefix or suffix.

TOC page numbers use a dotted flexible leader in the default `dotted` variant. Explicit `standard` and
`numbered` variants omit the leader but pin the page-number span with `ml-auto pl-3`, keeping numbers
on the right edge instead of immediately after variable-length title text.

### Pagination

`PaginateOptions` adds `coverFullPage`, `headingPageBreak`, `tocPageBreakBefore` and
`tocPageBreakAfter`. When `coverHeight > 0` and `coverFullPage` is true, page 1 is emitted
immediately with `includeCover: true`, no items and no TOC; TOC and body always begin on page 2.
Without a full-page cover, the cover still shares page 1 with the TOC when it fits.

The TOC starts a new page when `tocPageBreakBefore` is true and the current page already has content,
or when it would overflow the current page. When `tocPageBreakAfter` is true, body content starts on
the next page after the TOC. With both flags true, the TOC occupies its own page or pages.

`PageMeasurements` also carries `tocHeaderHeight`, `tocTrailingHeight` and display-order
`tocEntryHeights`, measured after fonts settle. `ReportTocSection` flattens the already depth-filtered
TOC while preserving level styling, so nested entries can be sliced without duplicating parents or
children. `paginateBlocks` packs these entries into contiguous half-open `TocRange` values. Every range
has a non-empty entry set; even a single entry taller than the available height advances the cursor.
Later pages render `目录（续）`. A small TOC retains the previous cover/body sharing behavior, and the
last range can share its page with body content unless `tocPageBreakAfter` is set. Synthetic fixtures
without per-entry measurements retain the atomic fallback.

`buildBlockPageMap` runs after all TOC and body ranges are known, so displayed TOC page numbers refer
to the final body pages. Bibliography used-height estimation charges only the actual `tocRange` on its
page rather than the complete TOC height.

While iterating blocks, a heading starts a new page when its own `style.page_break_before` is true
or when the document policy matches its level (`h1` for level 1, `h1_and_h2` for levels 1-2). The
break only fires when the current page already has body items or a TOC, so a heading at the top of a
fresh page never produces an empty page. A TOC-only page is preserved rather than discarded.

Before slicing a table onto the current page, the paginator checks compact tables whose complete
measured height is at most 90% of a fresh page. If the current page already has content and can hold
fewer than `min(6, ceil(rowCount * 0.6))` rows, the whole table moves to a fresh page rather than
leaving a small page-bottom fragment.

During row slicing, the remaining body height includes all remaining rows, `tfoot` and any continuation
footer, but is also compared independently with `notesHeight`. When the complete remaining body fits
and the notes alone do not, the final body slice contains every remaining row with `includeNotes:
false`; the next page receives a `notesOnly` slice. `ReportBlockItem` renders only `BlockFooterNotes`
for that slice and never repeats the table or caption. Bibliography packing accounts for these two
slices separately.

Column sizing measures text with 30px of horizontal chrome (24px padding plus rendering slack), and
headers of at most four characters render with `whitespace-nowrap`. After ideal-width water filling,
Grow columns absorb surplus only up to their pixel cap: the first column of a table with at most five
columns is capped at 34% of the available width, while other Grow columns use 38%. Any residue is
distributed by `idealPx` across columns that can still expand, including Fit numeric columns, preventing
a lone Grow text column from consuming most of a sparse table. If minimum widths still exceed the
container, the allocator reclaims at most the optional 6px atomic-column rendering slack before enabling
horizontal overflow; the real 24px padding and 56px atomic floor are preserved.

When a visual has `source_ids` but no `source_note`, `BlockFooterNotes` resolves up to three distinct
source names from bibliography `publisher` (falling back to `title`) and prepends them to the citation
markers. This prevents a bare `资料来源：[N]` when bibliography metadata can supply a readable name.

### User settings

`web/src/lib/pageChrome.ts` normalizes missing or invalid `heading_page_break` values to `none`,
merges the field through the same local override as header/footer settings, and persists it in
`quire.pageChrome.v1.<doc_id>`. `PageChromeDialog` exposes a single-select ToggleGroup with
`不强制`, `一级标题` and `一、二级标题`; page-chrome presets preserve the current heading-break
choice.

## 3. Invariants & Verification

- A custom-HTML cover page in paged/print mode contains only the cover, never the TOC, body blocks,
  header or footer; TOC/body start on page 2.
- Continuous custom covers keep natural height inside their full-bleed spacing wrapper and flow into
  the TOC without an additional `ReportCover` border or margin.
- `HeadingPageBreak` defaults to `none`; all three values round-trip through serde and the generated
  JSON Schema.
- `TocConfig.page_break_before` and `page_break_after` default to `false`; both round-trip through
  serde and the generated JSON Schema.
- `tocPageBreakBefore` and `tocPageBreakAfter` are applied independently; enabling both gives every
  TOC segment an exclusive page, including multi-page TOCs, and starts body content on the following
  page.
- Every visible TOC entry appears exactly once across contiguous ranges. Continuation ranges have no
  empty pages, duplicate parent entries or clipped entries, and their page numbers match the final
  body pages.
- Automatic heading breaks never emit a blank page when the heading is already at a page top.
- `TocConfig.variant` defaults to `dotted`; non-dotted page numbers still align to the right edge.
- Built-in cover variants never duplicate a subtitle's leading/trailing dashes.
- Compact single-page tables do not leave small page-bottom fragments; long notes never force the last
  data rows into a continuation solely to make room for those notes.
- Four-character headers receive single-line protection, and a sparse table's lone Grow first column
  stays within its cap while Fit columns share the remaining space.
- A `notesOnly` slice renders no table or caption, and missing source notes use readable bibliography
  names when available.
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
bun run test          # pagination.test.ts + reportRendering.test.tsx cover layout/rendering contracts
bun run typecheck
bun run lint
bun run build
playwright-cli run-code --filename=web/e2e/cover-and-heading-breaks.js
playwright-cli run-code --filename=web/e2e/toc-pagination.js
```

The Playwright scenario asserts the continuous-versus-paged cover difference, hidden page-1
header/footer, TOC starting on page 2, browser-side sanitization, and H1 page-break switching via
`PageChromeDialog` without blank pages.
