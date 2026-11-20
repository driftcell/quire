# CROSS-BLOCK ANNOTATION RANGES

## Metadata

- **Status:** Accepted
- **Date:** 2026-11-20
- **Promoted To:** `../../docs/RUNTIME_COMMENT_WORKFLOW.md`

## 1. Context & Problem

Comments were anchored to one block and the browser reconstructed quote highlights with
`textContent.indexOf` plus injected `<mark>`/button nodes. That failed for cross-paragraph and
media-spanning selections, and DOM injection inside complex report content could disturb table
columns or image rendering. Native selection also painted a blue film over images and canvases.

The product needs one annotation range that can cross paragraphs, headings, tables, images and
ECharts blocks without changing the canonical report DOM.

## 2. Rejected Options & Why

- **Keep single-block comments and split ranges into multiple threads.** Rejected: one user gesture
  would create several review items with duplicate bodies, and agents would lose the "whole range"
  intent.
- **Inject `<mark>` wrappers or absolutely positioned overlay rectangles per text range.** Rejected:
  wrappers mutate table markup and break fixed colgroups; overlay rectangles drift during paging,
  scaling, font loading and scrolling, and cannot participate in native text selection.
- **Use `textContent.indexOf` for highlight recovery.** Rejected: table tabs/newlines and block
  whitespace make offsets diverge from the DOM Range model, especially across page boundaries.
- **Store arbitrary character offsets only.** Rejected: protocol revisions and block edits make raw
  offsets brittle; quote + prefix/suffix and ordered block IDs remain resilient and human-readable.
- **Allow cross-block anchors without a start/end quote.** Rejected: the UI could not restore precise
  endpoint text after reload or trigger the Agent with the actual referenced fragments.

## 3. Chosen Approach & Trade-offs

`CommentThread` keeps `block_id` as the start/section anchor and adds `end_block_id`, ordered
`anchor_blocks`, and optional `end_quote`. The store persists these in `report_comment_threads` and
computes `anchor_detached` from any surviving anchor.

The browser captures a DOM `Range`, derives endpoint text with `Range.toString()` offsets, and builds
a summary with media placeholders for intermediate blocks. Persistent text highlights use
`CSS.highlights` + `::highlight()`; non-text blocks use a non-layout-affecting pseudo-element
outline. Media elements use `user-select: none` in both report CSS and custom-HTML shadow roots.

The cost is a wider protocol/migration surface and a browser capability check. In exchange,
annotations stay precise across mixed content, report DOM remains unchanged, and agents receive an
explicit start/end/media range.
