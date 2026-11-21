import type { HeadingPageBreak } from "@/lib/pageChrome";
import type { ReportBlock } from "@/lib/document";

export interface PageSlice {
  block: ReportBlock;
  /** For oversized tables split across pages. */
  rowStart?: number;
  rowEnd?: number;
  totalRows?: number;
  includeSummary?: boolean;
  includeNotes?: boolean;
  /** Render only the table's footer notes after its final body slice. */
  notesOnly?: boolean;
  continuation?: boolean;
}

export interface BibliographyRange {
  /** Half-open range `[start, end)` into the full bibliography entry list. */
  start: number;
  end: number;
}

export interface A4Page {
  pageNumber: number;
  items: PageSlice[];
  includeCover?: boolean;
  includeToc?: boolean;
  bibliographyRange?: BibliographyRange;
}

export interface TableMeasurement {
  theadHeight: number;
  tfootHeight: number;
  rowHeights: number[];
  /** Measured top-only table chrome: figure margins, caption and thead. */
  chromeHeight?: number;
  /** Footer notes rendered only by the final table slice. */
  notesHeight?: number;
}

export interface PageMeasurements {
  blockHeights: Record<string, number>;
  tableMeasurements: Record<string, TableMeasurement>;
  coverHeight: number;
  tocHeight: number;
  /** Measured first-page bibliography header including its top spacing. */
  bibliographyHeaderHeight: number;
  /** Per-item heights including the 14px `space-y-3.5` gap. */
  bibliographyItemHeights: number[];
}

export interface PaginateOptions {
  /** Protocol default is `true`; only an explicit `false` may share the last page. */
  referencesPageBreakBefore?: boolean;
  /** Start the TOC on a new page after the cover. */
  tocPageBreakBefore?: boolean;
  /** Start body content on a new page after the TOC. */
  tocPageBreakAfter?: boolean;
  /** Give a custom-HTML cover its own first page in paged/print mode. */
  coverFullPage?: boolean;
  /** Automatic page break before level-1 and/or level-2 headings. */
  headingPageBreak?: HeadingPageBreak;
}

export const A4_PAGE_WIDTH_PX = 794;
export const A4_PAGE_HEIGHT_PX = 1123;
export const A4_PADDING_X_PX = 56;
export const A4_PADDING_Y_PX = 56;
export const A4_HEADER_HEIGHT_PX = 28;
export const A4_FOOTER_HEIGHT_PX = 32;
export const A4_PAGE_SAFETY_PX = 8;
export const A4_CONTENT_WIDTH_PX = A4_PAGE_WIDTH_PX - A4_PADDING_X_PX * 2;
export const A4_CONTENT_HEIGHT_PX =
  A4_PAGE_HEIGHT_PX - A4_PADDING_Y_PX * 2 - A4_PAGE_SAFETY_PX;

const DEFAULT_ROW_HEIGHT_PX = 34;
const DEFAULT_TABLE_CAPTION_PX = 48;
const CONTINUATION_FOOTER_PX = 28;

function tableChromeHeight(
  blockId: string,
  metrics: TableMeasurement | undefined,
  blockHeights: Record<string, number>,
): number {
  const fallback = DEFAULT_TABLE_CAPTION_PX + (metrics?.theadHeight ?? 40);
  if (metrics?.chromeHeight !== undefined && metrics.chromeHeight > 0) {
    return Math.max(fallback, metrics.chromeHeight);
  }
  const blockHeight = blockHeights[blockId];
  if (metrics && blockHeight !== undefined && blockHeight > 0) {
    const rowsHeight = metrics.rowHeights.reduce((sum, row) => sum + row, 0);
    return Math.max(
      fallback,
      blockHeight -
        rowsHeight -
        metrics.tfootHeight -
        (metrics.notesHeight ?? 0),
    );
  }
  return fallback;
}

function fitTableRows(
  rowCount: number,
  cursor: number,
  available: number,
  metrics: TableMeasurement | undefined,
  tfootHeight: number,
  notesHeight: number,
): { count: number; height: number } {
  let height = 0;
  let count = 0;
  while (cursor + count < rowCount) {
    const rowHeight =
      metrics?.rowHeights[cursor + count] ?? DEFAULT_ROW_HEIGHT_PX;
    const isFinalRow = cursor + count === rowCount - 1;
    const extraBottom = isFinalRow
      ? tfootHeight + notesHeight + (cursor > 0 ? CONTINUATION_FOOTER_PX : 0)
      : CONTINUATION_FOOTER_PX;
    if (height + rowHeight + extraBottom > available) {
      break;
    }
    height += rowHeight;
    count += 1;
  }
  return { count, height };
}

/**
 * Bin-pack blocks into fixed A4 pages using real per-block and per-row DOM measurements.
 * - Page 1 deducts Cover height from available content height.
 * - The inline TOC follows the cover, sharing the page when it fits. Protocol
 *   `page_break_before`/`page_break_after` can force the TOC and body onto
 *   separate pages.
 * - ECharts and CustomHtml blocks are always atomic and never split across pages.
 * - Headings enforce keep-with-next when the following block doesn't fit.
 * - Oversized tables slice rows by measured row height while repeating <thead>,
 *   enforcing a minimum 2-row orphan guard at page bottoms.
 * - Bibliography requests a new page when `referencesPageBreakBefore` is true
 *   (the protocol default) or when its header and first entry do not fit. Entries
 *   are then greedily sliced into contiguous `bibliographyRange` pages.
 */
export function paginateBlocks(
  blocks: ReportBlock[],
  measurements: PageMeasurements,
  pageHeight: number = A4_CONTENT_HEIGHT_PX,
  options: PaginateOptions = {},
): A4Page[] {
  const { blockHeights, tableMeasurements, coverHeight, tocHeight } =
    measurements;

  const fullPageCover = options.coverFullPage ?? false;
  const headingPageBreak = options.headingPageBreak ?? "none";
  const tocPageBreakBefore = options.tocPageBreakBefore ?? false;
  const tocPageBreakAfter = options.tocPageBreakAfter ?? false;

  const pages: A4Page[] = [];
  let currentItems: PageSlice[] = [];
  let currentIncludeToc = false;
  let usedHeight = 0;

  if (coverHeight > 0 && fullPageCover) {
    pages.push({
      pageNumber: 1,
      items: [],
      includeCover: true,
      includeToc: false,
    });
  } else if (coverHeight > 0) {
    usedHeight = Math.min(coverHeight, pageHeight * 0.75);
  }

  const pushPage = () => {
    if (currentItems.length > 0 || currentIncludeToc || pages.length === 0) {
      pages.push({
        pageNumber: pages.length + 1,
        items: currentItems,
        includeCover: pages.length === 0 && coverHeight > 0,
        includeToc: currentIncludeToc,
      });
      currentItems = [];
      currentIncludeToc = false;
      usedHeight = 0;
    }
  };

  if (tocHeight > 0) {
    if (
      usedHeight > 0 &&
      (tocPageBreakBefore || usedHeight + tocHeight > pageHeight)
    ) {
      pushPage();
    }
    currentIncludeToc = true;
    usedHeight += tocHeight;
    if (tocPageBreakAfter) {
      pushPage();
    }
  }

  for (let i = 0; i < blocks.length; i += 1) {
    const block = blocks[i];
    if (!block) continue;

    const autoHeadingBreak =
      block.type === "heading" &&
      ((headingPageBreak === "h1" && block.level === 1) ||
        (headingPageBreak === "h1_and_h2" &&
          block.level !== undefined &&
          block.level <= 2));
    if (
      (block.style?.page_break_before || autoHeadingBreak) &&
      (currentItems.length > 0 || currentIncludeToc)
    ) {
      pushPage();
    }

    const measured = Math.max(24, blockHeights[block.id] ?? 120);

    if (block.type === "heading") {
      const nextBlock = blocks[i + 1];
      const nextHeight = nextBlock
        ? nextBlock.type === "table"
          ? tableChromeHeight(
              nextBlock.id,
              tableMeasurements[nextBlock.id],
              blockHeights,
            ) +
            2 * DEFAULT_ROW_HEIGHT_PX
          : Math.max(24, blockHeights[nextBlock.id] ?? 80)
        : 0;
      if (
        (currentItems.length > 0 || currentIncludeToc) &&
        usedHeight + measured + nextHeight > pageHeight
      ) {
        pushPage();
      }
      currentItems.push({ block });
      usedHeight += measured;
      continue;
    }

    if (block.type !== "table" || usedHeight + measured <= pageHeight) {
      if (
        (currentItems.length > 0 || currentIncludeToc) &&
        usedHeight + measured > pageHeight
      ) {
        pushPage();
      }
      currentItems.push({ block });
      usedHeight += measured;
      continue;
    }

    // Oversized table: slice rows using real measured row heights.
    const rows = Array.isArray(block.rows) ? block.rows : [];
    if (rows.length === 0) {
      if (currentItems.length > 0 || currentIncludeToc) {
        pushPage();
      }
      currentItems.push({ block });
      usedHeight += measured;
      continue;
    }

    const tableMetrics = tableMeasurements[block.id];
    const tfootHeight = tableMetrics ? tableMetrics.tfootHeight : 0;
    const notesHeight = tableMetrics?.notesHeight ?? 0;
    const chromeHeight = tableChromeHeight(
      block.id,
      tableMetrics,
      blockHeights,
    );

    // A table that fits comfortably on one fresh page should not leave a small
    // fragment at the bottom of the current page merely because a few rows fit.
    if (
      (currentItems.length > 0 || currentIncludeToc) &&
      measured <= pageHeight * 0.9
    ) {
      const available = pageHeight - usedHeight - chromeHeight;
      const { count } = fitTableRows(
        rows.length,
        0,
        available,
        tableMetrics,
        tfootHeight,
        notesHeight,
      );
      if (count < Math.min(6, Math.ceil(rows.length * 0.6))) {
        pushPage();
        currentItems.push({ block });
        usedHeight += measured;
        continue;
      }
    }

    let cursor = 0;
    while (cursor < rows.length) {
      const available = pageHeight - usedHeight - chromeHeight;
      const continuationFooterHeight = cursor > 0 ? CONTINUATION_FOOTER_PX : 0;
      const remainingRowsHeight = rows
        .slice(cursor)
        .reduce(
          (sum, _row, offset) =>
            sum +
            (tableMetrics?.rowHeights[cursor + offset] ??
              DEFAULT_ROW_HEIGHT_PX),
          0,
        );
      const remainingBodyHeight =
        remainingRowsHeight + tfootHeight + continuationFooterHeight;

      // Never sacrifice the last data rows just to keep long notes alongside
      // them. Keep the complete remaining body here and continue notes alone.
      if (
        remainingBodyHeight <= available &&
        remainingBodyHeight + notesHeight > available
      ) {
        currentItems.push({
          block,
          rowStart: cursor,
          rowEnd: rows.length,
          totalRows: rows.length,
          includeSummary: true,
          includeNotes: false,
          continuation: cursor > 0,
        });
        usedHeight += chromeHeight + remainingBodyHeight;
        pushPage();
        currentItems.push({ block, notesOnly: true });
        usedHeight += notesHeight;
        break;
      }

      const { count, height: accHeight } = fitTableRows(
        rows.length,
        cursor,
        available,
        tableMetrics,
        tfootHeight,
        notesHeight,
      );
      const remainingRows = rows.length - cursor;
      const minRowsToKeepOnCurrentPage = Math.min(2, remainingRows);

      if (
        count < minRowsToKeepOnCurrentPage &&
        (currentItems.length > 0 || currentIncludeToc)
      ) {
        pushPage();
        continue;
      }

      let take = Math.max(1, count);
      if (remainingRows > take && remainingRows - take === 1 && take >= 3) {
        take -= 1;
      }
      const pageAccHeight =
        count > take
          ? accHeight -
            (tableMetrics?.rowHeights[cursor + take] ?? DEFAULT_ROW_HEIGHT_PX)
          : accHeight;
      const nextCursor = Math.min(rows.length, cursor + take);
      const isLast = nextCursor === rows.length;
      currentItems.push({
        block,
        rowStart: cursor,
        rowEnd: nextCursor,
        totalRows: rows.length,
        includeSummary: isLast,
        continuation: cursor > 0,
      });
      usedHeight +=
        chromeHeight +
        pageAccHeight +
        (isLast
          ? tfootHeight + notesHeight + continuationFooterHeight
          : CONTINUATION_FOOTER_PX);
      cursor = nextCursor;
      if (!isLast) {
        pushPage();
      }
    }
  }

  pushPage();

  appendBibliographyPages(
    pages,
    measurements,
    pageHeight,
    options.referencesPageBreakBefore ?? true,
  );

  return pages;
}

/** Derive the compact continuation header from the measured first-page header. */
function continuedBibliographyHeaderHeight(firstHeaderHeight: number): number {
  const firstTopSpacing = 40 + 1 + 24; // mt-10 + border-t + pt-6
  const continuedTopSpacing = 8; // pt-2
  return Math.max(
    48,
    firstHeaderHeight - (firstTopSpacing - continuedTopSpacing),
  );
}

function bibliographyItemsForPage(
  itemHeights: number[],
  start: number,
  availableHeight: number,
): number {
  let usedHeight = 0;
  let count = 0;
  while (start + count < itemHeights.length) {
    const itemHeight = itemHeights[start + count] ?? 0;
    if (count > 0 && usedHeight + itemHeight > availableHeight) {
      break;
    }
    usedHeight += itemHeight;
    count += 1;
  }
  return Math.max(1, count);
}

function estimatedPageUsedHeight(
  page: A4Page,
  measurements: PageMeasurements,
  pageHeight: number,
): number {
  const { blockHeights, tableMeasurements, coverHeight, tocHeight } =
    measurements;
  return page.items.reduce(
    (sum, slice) => {
      if (slice.notesOnly) {
        return sum + (tableMeasurements[slice.block.id]?.notesHeight ?? 0);
      }
      if (slice.rowStart !== undefined && slice.rowEnd !== undefined) {
        const metrics = tableMeasurements[slice.block.id];
        const rowsCost = metrics
          ? metrics.rowHeights
              .slice(slice.rowStart, slice.rowEnd)
              .reduce((a, b) => a + b, 0)
          : (slice.rowEnd - slice.rowStart) * DEFAULT_ROW_HEIGHT_PX;
        return (
          sum +
          tableChromeHeight(slice.block.id, metrics, blockHeights) +
          rowsCost +
          (slice.includeSummary
            ? (metrics?.tfootHeight ?? 0) +
              (slice.includeNotes === false ? 0 : (metrics?.notesHeight ?? 0)) +
              (slice.continuation ? CONTINUATION_FOOTER_PX : 0)
            : CONTINUATION_FOOTER_PX)
        );
      }
      return sum + (blockHeights[slice.block.id] ?? 80);
    },
    (page.includeCover ? Math.min(coverHeight, pageHeight * 0.75) : 0) +
      (page.includeToc ? tocHeight : 0),
  );
}

function appendBibliographyPages(
  pages: A4Page[],
  measurements: PageMeasurements,
  pageHeight: number,
  mustBreak: boolean,
) {
  const { bibliographyHeaderHeight, bibliographyItemHeights } = measurements;
  if (bibliographyItemHeights.length === 0) {
    return;
  }
  const lastPage = pages[pages.length - 1];
  if (!lastPage) {
    return;
  }

  const lastPageUsed = estimatedPageUsedHeight(
    lastPage,
    measurements,
    pageHeight,
  );
  const hasContent =
    Boolean(lastPage.includeCover) ||
    Boolean(lastPage.includeToc) ||
    lastPage.items.length > 0;
  const firstItemHeight = bibliographyItemHeights[0] ?? 0;

  let page = lastPage;
  let pageUsed = lastPageUsed;
  if (
    hasContent &&
    (mustBreak ||
      lastPageUsed + bibliographyHeaderHeight + firstItemHeight > pageHeight)
  ) {
    page = {
      pageNumber: pages.length + 1,
      items: [],
    };
    pages.push(page);
    pageUsed = 0;
  }

  let cursor = 0;
  while (cursor < bibliographyItemHeights.length) {
    const headerHeight =
      cursor === 0
        ? bibliographyHeaderHeight
        : continuedBibliographyHeaderHeight(bibliographyHeaderHeight);
    const available = pageHeight - pageUsed - headerHeight;
    const take = bibliographyItemsForPage(
      bibliographyItemHeights,
      cursor,
      available,
    );
    page.bibliographyRange = { start: cursor, end: cursor + take };
    cursor += take;
    if (cursor < bibliographyItemHeights.length) {
      page = {
        pageNumber: pages.length + 1,
        items: [],
      };
      pages.push(page);
      pageUsed = 0;
    }
  }
}

/**
 * Map every block to the first page where it appears. Inline TOC page numbers
 * must come from this packed result, not from heading order or revision data.
 */
export function buildBlockPageMap(pages: A4Page[]): Map<string, number> {
  const map = new Map<string, number>();
  for (const page of pages) {
    for (const slice of page.items) {
      if (!map.has(slice.block.id)) {
        map.set(slice.block.id, page.pageNumber);
      }
    }
  }
  return map;
}
