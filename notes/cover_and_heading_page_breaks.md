# COVER AND HEADING PAGE BREAKS

## Metadata

- **Status:** Accepted
- **Date:** 2026-11-20
- **Promoted To:** `../docs/REPORT_COVER_AND_HEADING_BREAKS.md`

## 1. Context & Problem

Agent-authored `cover.custom_html` previously flowed through the same margin wrapper in continuous
and paged/print output. A short custom cover therefore shared page 1 with the TOC and body, and the
first PDF page showed a header and footer. Built-in covers are intentionally inline page headers and
must keep working that way.

Heading pagination was per-block only via `style.page_break_before`. Agents and users needed a
document-level switch for starting H1 (or H1/H2) sections on a new page without manually marking
every heading, and without risking blank pages when a heading is already at the top of a page.

## 2. Rejected Options & Why

- **CSS-only page breaks for custom covers.** `page-break-before: always` and print CSS cannot
  suppress the renderer's fixed A4 header/footer row or guarantee that the inline TOC measurement
  excludes page 1. The paged viewer packs explicit A4 pages, so relying on browser fragmentation
  would desynchronize page numbers from the TOC and print export.
- **Give every cover its own page.** Built-in `classic` / `modern` / `minimal` / `split` covers are
  compact headers that intentionally share page 1 with the TOC. Isolating them would change existing
  reports and leave large white gaps.
- **Regex-only HTML sanitization.** Attribute values can be entity-encoded, contain control
  characters or live on nested SVG elements; a tag/attribute regex is easy to bypass and hard to
  keep correct. The browser's own HTML parser is the only reliable way to normalize what will be
  assigned to `innerHTML`.
- **Per-heading `page_break_before` authored by agents.** This requires touching every heading and
  drifts when sections are inserted later. A document-level policy is idempotent and can be edited
  by humans in the same dialog as header/footer settings.
- **Frontend-only heading policy.** A local-only switch would be invisible to MCP agents and would
  not survive export from another client. The policy belongs in `PageChromeConfig`.

## 3. Chosen Approach & Trade-offs

`HeadingPageBreak` is part of the report protocol and `PageChromeConfig`, so agents configure it
through `configure_page_chrome` and users configure it through `PageChromeDialog`; both converge on
the same local/document configuration path.

Custom HTML covers are isolated only when the cover is explicitly custom HTML and the viewer is in
paged or print mode. `ReportViewer` forwards `coverFullPage` to the paginator and `ReportPage`, which
suppresses page chrome and centers the cover in the remaining A4 height. Continuous mode keeps the
natural flow with a separator.

Sanitization uses a native `DOMParser`, strips executable elements/handlers/URLs, and leaves layout
markup intact. The accepted cost is that exotic SVG or embedded interactive content is removed
rather than repaired; authors should use static cover markup.

Bun has no DOM, so the sanitizer unit test polyfills `DOMParser` with `happy-dom`; production code
always uses the browser-native implementation. The test fixture is a dev dependency, not shipped in
the bundle.
