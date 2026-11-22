# TOC PAGINATION

## Metadata

- **Status:** Accepted
- **Date:** 2026-11-22
- **Promoted To:** `../docs/REPORT_COVER_AND_HEADING_BREAKS.md`

## 1. Context & Problem

Paged reports treat the complete TOC as one atomic item. When its measured height exceeds the A4
content height, moving it to a fresh page cannot prevent clipping. A real long report produced 47 TOC
entries and lost nine entries beyond the page boundary. TOC page numbers must continue to come from
the final packed body pages, and bibliography placement must account for each TOC segment rather than
the original whole-TOC height.

## 2. Rejected Options & Why

- **Use scrolling or keep `overflow: hidden`.** Scrolling is not available in print/PDF, while hidden
  overflow is the current data-loss behavior. Neither creates printable continuation pages.
- **Split only top-level TOC nodes.** Nested entries can exceed a page even when each top-level group
  is small. Splitting at this granularity either overflows or wastes large page regions.
- **Assign fixed entry heights.** Titles wrap at different widths and levels have different type and
  padding. Fixed heights reintroduce overflow and do not respect font loading or theme changes.
- **Reserve a fixed number of TOC pages before measuring entries.** Empty space and page counts become
  incorrect as heading depth and title lengths change.

## 3. Chosen Approach & Trade-offs

Flatten the already depth-filtered TOC into display-order entries while retaining each node's level
styling. Measure the TOC header, trailing spacing and every rendered entry after fonts settle. Pack
entries into contiguous ranges, render later ranges as `目录（续）`, and leave the final range available
for body sharing unless `tocPageBreakAfter` is set. Existing small-TOC behavior and atomic fallback
fixtures remain supported.

The TOC and body still share the same paginator, so `buildBlockPageMap` supplies final body page
numbers after all TOC ranges are known. Bibliography used-height estimation uses the actual range on
its page, not the full TOC height.

The trade-off is a flatter list DOM instead of nested lists. Visual hierarchy remains explicit through
level classes, while the simpler structure makes per-entry measurement and exact range slicing
reliable. Entry gaps are included with a small conservative allowance at range boundaries.
