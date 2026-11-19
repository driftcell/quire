import type { ReportBlock } from "@/lib/document";

export interface PageSlice {
  block: ReportBlock;
  /** For oversized tables split across pages. */
  rowStart?: number;
  rowEnd?: number;
  totalRows?: number;
  includeSummary?: boolean;
  continuation?: boolean;
}

export interface A4Page {
  pageNumber: number;
  items: PageSlice[];
  includeCover?: boolean;
  includeBibliography?: boolean;
}

export interface TableMeasurement {
  theadHeight: number;
  tfootHeight: number;
  rowHeights: number[];
  /** Measured non-row table chrome: figure margins, caption, thead and notes. */
  chromeHeight?: number;
}

export interface PageMeasurements {
  blockHeights: Record<string, number>;
  tableMeasurements: Record<string, TableMeasurement>;
  coverHeight: number;
  bibliographyHeight: number;
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
    return Math.max(fallback, blockHeight - rowsHeight - metrics.tfootHeight);
  }
  return fallback;
}

/**
 * Bin-pack blocks into fixed A4 pages using real per-block and per-row DOM measurements.
 * - Page 1 deducts Cover height from available content height.
 * - ECharts and CustomHtml blocks are always atomic and never split across pages.
 * - Headings enforce keep-with-next when the following block doesn't fit.
 * - Oversized tables slice rows by measured row height while repeating <thead>,
 *   enforcing a minimum 2-row orphan guard at page bottoms.
 * - Bibliography moves to a clean trailing page if it doesn't fit on the last content page.
 */
export function paginateBlocks(
  blocks: ReportBlock[],
  measurements: PageMeasurements,
  pageHeight: number = A4_CONTENT_HEIGHT_PX,
): A4Page[] {
  const { blockHeights, tableMeasurements, coverHeight, bibliographyHeight } =
    measurements;

  const pages: A4Page[] = [];
  let currentItems: PageSlice[] = [];
  let usedHeight =
    coverHeight > 0 ? Math.min(coverHeight, pageHeight * 0.75) : 0;

  const pushPage = () => {
    if (currentItems.length > 0 || pages.length === 0) {
      pages.push({
        pageNumber: pages.length + 1,
        items: currentItems,
        includeCover: pages.length === 0 && coverHeight > 0,
      });
      currentItems = [];
      usedHeight = 0;
    }
  };

  for (let i = 0; i < blocks.length; i += 1) {
    const block = blocks[i];
    if (!block) continue;

    if (block.style?.page_break_before && currentItems.length > 0) {
      pushPage();
    }

    const measured = Math.max(24, blockHeights[block.id] ?? 120);

    if (block.type === "heading") {
      const nextBlock = blocks[i + 1];
      const nextHeight = nextBlock
        ? Math.min(140, blockHeights[nextBlock.id] ?? 80)
        : 0;
      if (
        currentItems.length > 0 &&
        usedHeight + measured + nextHeight > pageHeight
      ) {
        pushPage();
      }
      currentItems.push({ block });
      usedHeight += measured;
      continue;
    }

    if (block.type !== "table" || usedHeight + measured <= pageHeight) {
      if (currentItems.length > 0 && usedHeight + measured > pageHeight) {
        pushPage();
      }
      currentItems.push({ block });
      usedHeight += measured;
      continue;
    }

    // Oversized table: slice rows using real measured row heights.
    const rows = Array.isArray(block.rows) ? block.rows : [];
    if (rows.length === 0) {
      if (currentItems.length > 0) {
        pushPage();
      }
      currentItems.push({ block });
      usedHeight += measured;
      continue;
    }

    const tableMetrics = tableMeasurements[block.id];
    const tfootHeight = tableMetrics ? tableMetrics.tfootHeight : 0;
    const chromeHeight = tableChromeHeight(
      block.id,
      tableMetrics,
      blockHeights,
    );

    let cursor = 0;
    while (cursor < rows.length) {
      const available = pageHeight - usedHeight - chromeHeight;
      let accHeight = 0;
      let count = 0;

      while (cursor + count < rows.length) {
        const rowH =
          tableMetrics?.rowHeights[cursor + count] ?? DEFAULT_ROW_HEIGHT_PX;
        const isFinalRow = cursor + count === rows.length - 1;
        const extraBottom = isFinalRow ? tfootHeight : CONTINUATION_FOOTER_PX;
        if (accHeight + rowH + extraBottom > available) {
          break;
        }
        accHeight += rowH;
        count += 1;
      }

      const remainingRows = rows.length - cursor;
      const minRowsToKeepOnCurrentPage = Math.min(2, remainingRows);

      if (count < minRowsToKeepOnCurrentPage && currentItems.length > 0) {
        pushPage();
        continue;
      }

      const take = Math.max(1, count);
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
        accHeight +
        (isLast ? tfootHeight : CONTINUATION_FOOTER_PX);
      cursor = nextCursor;
      if (!isLast) {
        pushPage();
      }
    }
  }

  pushPage();

  if (bibliographyHeight > 0 && pages.length > 0) {
    const lastPage = pages[pages.length - 1];
    if (lastPage) {
      const lastPageUsed = lastPage.items.reduce(
        (sum, slice) => {
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
                ? (metrics?.tfootHeight ?? 0)
                : CONTINUATION_FOOTER_PX)
            );
          }
          return sum + (blockHeights[slice.block.id] ?? 80);
        },
        lastPage.includeCover ? Math.min(coverHeight, pageHeight * 0.75) : 0,
      );

      if (lastPageUsed + bibliographyHeight <= pageHeight) {
        lastPage.includeBibliography = true;
      } else {
        pages.push({
          pageNumber: pages.length + 1,
          items: [],
          includeBibliography: true,
        });
      }
    }
  }

  return pages;
}
