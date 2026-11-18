import type { ReportBlock } from "@/lib/document";

export interface PageSlice {
  block: ReportBlock;
  /** For oversized tables split across pages. */
  rowStart?: number;
  rowEnd?: number;
  includeSummary?: boolean;
  continuation?: boolean;
}

export interface A4Page {
  pageNumber: number;
  items: PageSlice[];
}

export const A4_PAGE_WIDTH_PX = 794;
export const A4_PAGE_HEIGHT_PX = 1123;
export const A4_PADDING_X_PX = 56;
export const A4_PADDING_Y_PX = 56;
export const A4_CONTENT_WIDTH_PX = A4_PAGE_WIDTH_PX - A4_PADDING_X_PX * 2;
export const A4_CONTENT_HEIGHT_PX =
  A4_PAGE_HEIGHT_PX - A4_PADDING_Y_PX * 2 - 32;

const DEFAULT_ROW_HEIGHT_PX = 34;
const DEFAULT_TABLE_CHROME_PX = 96;

/**
 * Bin-pack blocks into fixed A4 pages.
 * - ECharts and CustomHtml blocks are always atomic and never split across pages.
 * - Headings enforce keep-with-next when the following block doesn't fit.
 * - Oversized tables slice rows cleanly across pages while repeating headers.
 */
export function paginateBlocks(
  blocks: ReportBlock[],
  heights: Record<string, number>,
  pageHeight: number = A4_CONTENT_HEIGHT_PX,
): A4Page[] {
  const pages: A4Page[] = [];
  let currentItems: PageSlice[] = [];
  let usedHeight = 0;

  const pushPage = () => {
    if (currentItems.length > 0) {
      pages.push({ pageNumber: pages.length + 1, items: currentItems });
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

    const measured = Math.max(24, heights[block.id] ?? 120);

    // Keep heading attached to at least a small slice of the following block.
    if (block.type === "heading") {
      const nextBlock = blocks[i + 1];
      const nextHeight = nextBlock
        ? Math.min(140, heights[nextBlock.id] ?? 80)
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

    // Atomic blocks (ECharts, CustomHtml, short tables, paragraphs, quotes)
    if (block.type !== "table" || usedHeight + measured <= pageHeight) {
      if (currentItems.length > 0 && usedHeight + measured > pageHeight) {
        pushPage();
      }
      currentItems.push({ block });
      usedHeight += measured;
      continue;
    }

    // Oversized table: slice rows across pages and repeat headers on each page.
    const rows = Array.isArray(block.rows) ? block.rows : [];
    if (rows.length === 0) {
      if (currentItems.length > 0) {
        pushPage();
      }
      currentItems.push({ block });
      usedHeight += measured;
      continue;
    }

    const estRowHeight = Math.max(
      26,
      Math.round((measured - DEFAULT_TABLE_CHROME_PX) / rows.length) ||
        DEFAULT_ROW_HEIGHT_PX,
    );
    let cursor = 0;
    while (cursor < rows.length) {
      const available = pageHeight - usedHeight - DEFAULT_TABLE_CHROME_PX;
      const fitRows = Math.floor(available / estRowHeight);
      if (fitRows < 2 && currentItems.length > 0) {
        pushPage();
        continue;
      }
      const take = Math.max(1, fitRows);
      const nextCursor = Math.min(rows.length, cursor + take);
      const isLast = nextCursor === rows.length;
      currentItems.push({
        block,
        rowStart: cursor,
        rowEnd: nextCursor,
        includeSummary: isLast,
        continuation: cursor > 0,
      });
      usedHeight +=
        DEFAULT_TABLE_CHROME_PX + (nextCursor - cursor) * estRowHeight;
      cursor = nextCursor;
      if (!isLast) {
        pushPage();
      }
    }
  }

  pushPage();
  return pages;
}
