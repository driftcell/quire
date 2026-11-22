import { describe, expect, test } from "bun:test";
import { Window } from "happy-dom";
import type { ReportBlock } from "@/lib/document";
import { sanitizeCoverHtml } from "./ReportCover";
import {
  buildBlockPageMap,
  paginateBlocks,
  type A4Page,
  type PageMeasurements,
} from "./pagination";

// Bun has no DOM; production always uses the browser-native DOMParser.
const domWindow = new Window();
(globalThis as { DOMParser?: unknown }).DOMParser = domWindow.DOMParser;

function heading(id: string, level = 1): ReportBlock {
  return { id, type: "heading", level, text: id };
}

function paragraph(id: string): ReportBlock {
  return { id, type: "paragraph", text: id };
}

function tableBlock(id: string, rowCount: number): ReportBlock {
  return {
    id,
    type: "table",
    columns: [],
    rows: Array.from({ length: rowCount }, (_, index) => ({
      name: `row-${index}`,
    })),
  };
}

function measurements(
  overrides: Partial<PageMeasurements> = {},
): PageMeasurements {
  return {
    blockHeights: {},
    tableMeasurements: {},
    coverHeight: 0,
    tocHeight: 0,
    tocHeaderHeight: 0,
    tocTrailingHeight: 0,
    tocEntryHeights: [],
    bibliographyHeaderHeight: 0,
    bibliographyItemHeights: [],
    ...overrides,
  };
}

function pageAt(pages: A4Page[], index: number): A4Page {
  const page = pages[index];
  if (!page) {
    throw new Error(`missing page at index ${index}`);
  }
  return page;
}

describe("paginateBlocks with an inline TOC", () => {
  test("keeps the TOC on the cover page when it fits and maps headings to real pages", () => {
    const blocks = [
      heading("h1"),
      paragraph("p1"),
      heading("h2"),
      paragraph("p2"),
      paragraph("p3"),
    ];
    const pages = paginateBlocks(
      blocks,
      measurements({
        blockHeights: { h1: 40, p1: 100, h2: 40, p2: 100, p3: 100 },
        coverHeight: 120,
        tocHeight: 80,
      }),
      500,
    );

    expect(pages).toHaveLength(2);
    expect(pageAt(pages, 0).includeCover).toBe(true);
    expect(pageAt(pages, 0).includeToc).toBe(true);
    expect(pageAt(pages, 1).includeToc).toBe(false);

    const pageMap = buildBlockPageMap(pages);
    expect(pageMap.get("h1")).toBe(1);
    expect(pageMap.get("h2")).toBe(1);
    expect(pageMap.get("p3")).toBe(2);
  });

  test("starts a new page for the TOC when the cover leaves too little room", () => {
    const pages = paginateBlocks(
      [paragraph("p1")],
      measurements({
        blockHeights: { p1: 100 },
        coverHeight: 320,
        tocHeight: 200,
      }),
      500,
    );

    expect(pages).toHaveLength(2);
    expect(pageAt(pages, 0).includeCover).toBe(true);
    expect(pageAt(pages, 0).includeToc).toBe(false);
    expect(pageAt(pages, 0).items).toHaveLength(0);
    expect(pageAt(pages, 1).includeCover).toBe(false);
    expect(pageAt(pages, 1).includeToc).toBe(true);

    const pageMap = buildBlockPageMap(pages);
    expect(pageMap.get("p1")).toBe(2);
  });

  test("tocPageBreakBefore starts the TOC on a fresh page even when it fits", () => {
    const pages = paginateBlocks(
      [paragraph("p1")],
      measurements({
        blockHeights: { p1: 100 },
        coverHeight: 120,
        tocHeight: 80,
      }),
      500,
      { tocPageBreakBefore: true },
    );

    expect(pages).toHaveLength(2);
    expect(pageAt(pages, 0).includeCover).toBe(true);
    expect(pageAt(pages, 0).includeToc).toBe(false);
    expect(pageAt(pages, 0).items).toHaveLength(0);
    expect(pageAt(pages, 1).includeToc).toBe(true);
    expect(pageAt(pages, 1).items[0]?.block.id).toBe("p1");
  });

  test("tocPageBreakAfter starts body content on the page after the TOC", () => {
    const pages = paginateBlocks(
      [paragraph("p1")],
      measurements({
        blockHeights: { p1: 100 },
        coverHeight: 120,
        tocHeight: 80,
      }),
      500,
      { tocPageBreakAfter: true },
    );

    expect(pages).toHaveLength(2);
    expect(pageAt(pages, 0).includeCover).toBe(true);
    expect(pageAt(pages, 0).includeToc).toBe(true);
    expect(pageAt(pages, 0).items).toHaveLength(0);
    expect(pageAt(pages, 1).includeToc).toBe(false);
    expect(pageAt(pages, 1).items[0]?.block.id).toBe("p1");
  });

  test("both TOC page breaks make the TOC occupy its own page", () => {
    const pages = paginateBlocks(
      [paragraph("p1")],
      measurements({
        blockHeights: { p1: 100 },
        coverHeight: 120,
        tocHeight: 80,
      }),
      500,
      { tocPageBreakBefore: true, tocPageBreakAfter: true },
    );

    expect(pages).toHaveLength(3);
    expect(pageAt(pages, 0)).toMatchObject({
      includeCover: true,
      includeToc: false,
      items: [],
    });
    expect(pageAt(pages, 1)).toMatchObject({
      includeCover: false,
      includeToc: true,
      items: [],
    });
    expect(pageAt(pages, 2).includeToc).toBe(false);
    expect(pageAt(pages, 2).items[0]?.block.id).toBe("p1");
  });
  test("splits an oversized TOC into contiguous ranges and shifts body page numbers", () => {
    const pages = paginateBlocks(
      [heading("h1"), paragraph("p1")],
      measurements({
        blockHeights: { h1: 40, p1: 100 },
        tocHeight: 410,
        tocHeaderHeight: 60,
        tocTrailingHeight: 20,
        tocEntryHeights: [100, 80, 90, 60],
      }),
      250,
      { tocPageBreakAfter: true },
    );

    expect(pages).toHaveLength(4);
    expect(pageAt(pages, 0).tocRange).toEqual({ start: 0, end: 1 });
    expect(pageAt(pages, 1).tocRange).toEqual({ start: 1, end: 3 });
    expect(pageAt(pages, 2).tocRange).toEqual({ start: 3, end: 4 });
    for (const page of pages.slice(0, 3)) {
      expect(page.includeToc).toBe(true);
      expect(page.items).toHaveLength(0);
    }
    expect(pageAt(pages, 3).includeToc).toBe(false);
    expect(pageAt(pages, 3).items.map((slice) => slice.block.id)).toEqual([
      "h1",
      "p1",
    ]);
    const pageMap = buildBlockPageMap(pages);
    expect(pageMap.get("h1")).toBe(4);
    expect(pageMap.get("p1")).toBe(4);
  });

  test("keeps every oversized TOC page exclusive with both explicit page breaks", () => {
    const pages = paginateBlocks(
      [paragraph("p1")],
      measurements({
        blockHeights: { p1: 100 },
        coverHeight: 120,
        tocHeight: 410,
        tocHeaderHeight: 60,
        tocTrailingHeight: 20,
        tocEntryHeights: [100, 80, 90, 60],
      }),
      250,
      { tocPageBreakBefore: true, tocPageBreakAfter: true },
    );

    expect(pages).toHaveLength(5);
    expect(pageAt(pages, 0).includeCover).toBe(true);
    expect(pageAt(pages, 0).includeToc).toBe(false);
    for (const [index, expected] of [
      { start: 0, end: 1 },
      { start: 1, end: 3 },
      { start: 3, end: 4 },
    ].entries()) {
      const page = pageAt(pages, index + 1);
      expect(page.tocRange).toEqual(expected);
      expect(page.items).toHaveLength(0);
    }
    expect(pageAt(pages, 4).includeToc).toBe(false);
    expect(pageAt(pages, 4).items[0]?.block.id).toBe("p1");
  });

  test("keeps a measured small TOC on the cover page", () => {
    const pages = paginateBlocks(
      [paragraph("p1")],
      measurements({
        blockHeights: { p1: 100 },
        coverHeight: 50,
        tocHeight: 100,
        tocHeaderHeight: 20,
        tocTrailingHeight: 10,
        tocEntryHeights: [30, 40],
      }),
      500,
    );

    expect(pageAt(pages, 0).includeCover).toBe(true);
    expect(pageAt(pages, 0).tocRange).toEqual({ start: 0, end: 2 });
    expect(pageAt(pages, 0).items[0]?.block.id).toBe("p1");
  });

  test("always makes progress when one TOC entry exceeds the available height", () => {
    const pages = paginateBlocks(
      [],
      measurements({
        tocHeight: 400,
        tocHeaderHeight: 60,
        tocTrailingHeight: 20,
        tocEntryHeights: [300, 50],
      }),
      100,
    );

    expect(pages).toHaveLength(2);
    expect(pageAt(pages, 0).tocRange).toEqual({ start: 0, end: 1 });
    expect(pageAt(pages, 1).tocRange).toEqual({ start: 1, end: 2 });
  });

  test("uses only the actual TOC range when sharing a page with references", () => {
    const pages = paginateBlocks(
      [],
      measurements({
        tocHeight: 1_000,
        tocHeaderHeight: 60,
        tocTrailingHeight: 20,
        tocEntryHeights: [100, 80, 90, 60],
        bibliographyHeaderHeight: 20,
        bibliographyItemHeights: [30],
      }),
      250,
      { referencesPageBreakBefore: false },
    );

    expect(pages).toHaveLength(3);
    expect(pageAt(pages, 2).tocRange).toEqual({ start: 3, end: 4 });
    expect(pageAt(pages, 2).bibliographyRange).toEqual({ start: 0, end: 1 });
  });
});

describe("paginateBlocks references page break", () => {
  test("page_break_before=true moves references to a clean trailing page", () => {
    const pages = paginateBlocks(
      [paragraph("body")],
      measurements({
        blockHeights: { body: 100 },
        bibliographyHeaderHeight: 40,
        bibliographyItemHeights: [20, 20],
      }),
      500,
      { referencesPageBreakBefore: true },
    );

    expect(pages).toHaveLength(2);
    expect(pageAt(pages, 0).bibliographyRange).toBeUndefined();
    expect(pageAt(pages, 1).bibliographyRange).toEqual({ start: 0, end: 2 });
  });

  test("page_break_before=false shares the last page when references fit", () => {
    const pages = paginateBlocks(
      [paragraph("body")],
      measurements({
        blockHeights: { body: 100 },
        bibliographyHeaderHeight: 40,
        bibliographyItemHeights: [20, 20],
      }),
      500,
      { referencesPageBreakBefore: false },
    );

    expect(pages).toHaveLength(1);
    expect(pageAt(pages, 0).bibliographyRange).toEqual({ start: 0, end: 2 });
  });

  test("page_break_before=false still breaks when references overflow", () => {
    const pages = paginateBlocks(
      [paragraph("body")],
      measurements({
        blockHeights: { body: 100 },
        bibliographyHeaderHeight: 250,
        bibliographyItemHeights: [200, 200],
      }),
      500,
      { referencesPageBreakBefore: false },
    );

    expect(pages).toHaveLength(3);
    expect(pageAt(pages, 0).bibliographyRange).toBeUndefined();
    expect(pageAt(pages, 1).bibliographyRange).toEqual({ start: 0, end: 1 });
    expect(pageAt(pages, 2).bibliographyRange).toEqual({ start: 1, end: 2 });
  });

  test("splits an oversized bibliography into contiguous ranges", () => {
    const itemHeights = Array.from({ length: 15 }, () => 60);
    const pages = paginateBlocks(
      [paragraph("body")],
      measurements({
        blockHeights: { body: 100 },
        bibliographyHeaderHeight: 100,
        bibliographyItemHeights: itemHeights,
      }),
      500,
      { referencesPageBreakBefore: true },
    );

    const bibliographyPages = pages.filter(
      (page) => page.bibliographyRange !== undefined,
    );
    expect(bibliographyPages.length).toBeGreaterThanOrEqual(2);

    let nextStart = 0;
    for (const page of bibliographyPages) {
      const range = page.bibliographyRange;
      if (!range) {
        throw new Error("bibliography page without a range");
      }
      expect(range.start).toBe(nextStart);
      expect(range.end).toBeGreaterThan(range.start);
      expect(range.end).toBeLessThanOrEqual(15);
      nextStart = range.end;
    }
    expect(nextStart).toBe(15);
  });
});

describe("paginateBlocks heading keep-with-next", () => {
  test("moves a heading with a non-table block that no longer fits", () => {
    const pages = paginateBlocks(
      [paragraph("before"), heading("h2", 2), paragraph("next")],
      measurements({
        blockHeights: { before: 280, h2: 40, next: 220 },
      }),
      500,
    );

    expect(pages).toHaveLength(2);
    expect(pageAt(pages, 0).items.map((item) => item.block.id)).toEqual([
      "before",
    ]);
    expect(pageAt(pages, 1).items.map((item) => item.block.id)).toEqual([
      "h2",
      "next",
    ]);
  });
});

describe("paginateBlocks table widow row guard", () => {
  test("moves one row from a 17-row slice to avoid a 1-row continuation", () => {
    const pages = paginateBlocks(
      [tableBlock("wide-table", 18)],
      measurements({
        blockHeights: { "wide-table": 600 },
        tableMeasurements: {
          "wide-table": {
            theadHeight: 40,
            tfootHeight: 20,
            rowHeights: Array.from({ length: 18 }, () => 20),
            chromeHeight: 125,
          },
        },
      }),
      500,
    );

    expect(pages).toHaveLength(2);
    expect(pageAt(pages, 0).items[0]).toMatchObject({
      rowStart: 0,
      rowEnd: 16,
      totalRows: 18,
      continuation: false,
      includeSummary: false,
    });
    expect(pageAt(pages, 1).items[0]).toMatchObject({
      rowStart: 16,
      rowEnd: 18,
      totalRows: 18,
      continuation: true,
      includeSummary: true,
    });
  });
});

describe("paginateBlocks single-page table placement", () => {
  test("moves a compact table to a fresh page instead of leaving a page-bottom fragment", () => {
    const table = tableBlock("compact-table", 12);
    const pages = paginateBlocks(
      [paragraph("before"), table],
      measurements({
        blockHeights: { before: 350, "compact-table": 340 },
        tableMeasurements: {
          "compact-table": {
            theadHeight: 40,
            tfootHeight: 20,
            rowHeights: Array.from({ length: 12 }, () => 20),
            chromeHeight: 60,
            notesHeight: 20,
          },
        },
      }),
      500,
    );

    expect(pages).toHaveLength(2);
    expect(pageAt(pages, 0).items.map((item) => item.block.id)).toEqual([
      "before",
    ]);
    expect(pageAt(pages, 1).items).toHaveLength(1);
    expect(pageAt(pages, 1).items[0]).toMatchObject({ block: table });
    expect(pageAt(pages, 1).items[0]?.rowStart).toBeUndefined();
    expect(pageAt(pages, 1).items[0]?.rowEnd).toBeUndefined();
  });
});

describe("paginateBlocks continuation footer notes", () => {
  test("charges long notes only to the final slice and keeps that slice within the page", () => {
    const table = tableBlock("continued-table", 20);
    const pages = paginateBlocks(
      [table],
      measurements({
        blockHeights: { "continued-table": 688 },
        tableMeasurements: {
          "continued-table": {
            theadHeight: 40,
            tfootHeight: 20,
            rowHeights: Array.from({ length: 20 }, () => 20),
            chromeHeight: 88,
            notesHeight: 180,
          },
        },
      }),
      500,
    );

    expect(pages).toHaveLength(2);
    expect(pageAt(pages, 0).items[0]).toMatchObject({
      rowStart: 0,
      rowEnd: 18,
      includeSummary: false,
    });
    expect(pageAt(pages, 1).items[0]).toMatchObject({
      rowStart: 18,
      rowEnd: 20,
      continuation: true,
      includeSummary: true,
    });

    const finalSliceHeight = 88 + 2 * 20 + 20 + 180 + 28;
    expect(finalSliceHeight).toBeLessThanOrEqual(500);
  });

  test("keeps every remaining data row together and moves only oversized notes", () => {
    const pages = paginateBlocks(
      [tableBlock("notes-only-table", 20)],
      measurements({
        blockHeights: { "notes-only-table": 688 },
        tableMeasurements: {
          "notes-only-table": {
            theadHeight: 40,
            tfootHeight: 20,
            rowHeights: Array.from({ length: 20 }, () => 20),
            chromeHeight: 88,
            notesHeight: 180,
          },
        },
      }),
      400,
    );

    expect(pages).toHaveLength(3);
    expect(pageAt(pages, 0).items[0]).toMatchObject({
      rowStart: 0,
      rowEnd: 14,
      includeSummary: false,
    });
    expect(pageAt(pages, 1).items).toHaveLength(1);
    expect(pageAt(pages, 1).items[0]).toMatchObject({
      rowStart: 14,
      rowEnd: 20,
      continuation: true,
      includeSummary: true,
      includeNotes: false,
    });
    expect(pageAt(pages, 1).items[0]?.notesOnly).toBeUndefined();
    expect(pageAt(pages, 2).items).toHaveLength(1);
    expect(pageAt(pages, 2).items[0]).toMatchObject({ notesOnly: true });
    expect(pageAt(pages, 2).items[0]?.rowStart).toBeUndefined();
    expect(pageAt(pages, 2).items[0]?.rowEnd).toBeUndefined();
  });
});

describe("paginateBlocks cover page isolation", () => {
  test("coverFullPage puts only the cover on page 1 and starts TOC/body on page 2", () => {
    const pages = paginateBlocks(
      [heading("h1"), paragraph("p1")],
      measurements({
        blockHeights: { h1: 40, p1: 100 },
        coverHeight: 300,
        tocHeight: 80,
      }),
      500,
      { coverFullPage: true },
    );

    expect(pages).toHaveLength(2);
    expect(pageAt(pages, 0)).toMatchObject({
      includeCover: true,
      includeToc: false,
      items: [],
    });
    expect(pageAt(pages, 1).includeCover).toBe(false);
    expect(pageAt(pages, 1).includeToc).toBe(true);

    const pageMap = buildBlockPageMap(pages);
    expect(pageMap.get("h1")).toBe(2);
    expect(pageMap.get("p1")).toBe(2);
  });

  test("coverFullPage without body blocks leaves a single cover page", () => {
    const pages = paginateBlocks([], measurements({ coverHeight: 300 }), 500, {
      coverFullPage: true,
    });

    expect(pages).toHaveLength(1);
    expect(pageAt(pages, 0).includeCover).toBe(true);
    expect(pageAt(pages, 0).items).toHaveLength(0);
  });
});

describe("paginateBlocks automatic heading breaks", () => {
  test("h1 starts every level-1 heading on a fresh page", () => {
    const pages = paginateBlocks(
      [heading("h1a"), paragraph("p1"), heading("h1b"), paragraph("p2")],
      measurements({
        blockHeights: { h1a: 40, p1: 100, h1b: 40, p2: 100 },
      }),
      500,
      { headingPageBreak: "h1" },
    );

    expect(pages).toHaveLength(2);
    const pageMap = buildBlockPageMap(pages);
    expect(pageMap.get("h1a")).toBe(1);
    expect(pageMap.get("h1b")).toBe(2);
  });

  test("h1_and_h2 breaks level-2 headings but leaves level-3 headings inline", () => {
    const pages = paginateBlocks(
      [
        heading("h1"),
        paragraph("p1"),
        heading("h2", 2),
        paragraph("p2"),
        heading("h3", 3),
        paragraph("p3"),
      ],
      measurements({
        blockHeights: {
          h1: 40,
          p1: 100,
          h2: 40,
          p2: 100,
          h3: 40,
          p3: 100,
        },
      }),
      500,
      { headingPageBreak: "h1_and_h2" },
    );

    expect(pages).toHaveLength(2);
    const pageMap = buildBlockPageMap(pages);
    expect(pageMap.get("h2")).toBe(2);
    expect(pageMap.get("h3")).toBe(2);
  });

  test("never emits a blank page when the heading is already at the page top", () => {
    const pages = paginateBlocks(
      [heading("h1")],
      measurements({ blockHeights: { h1: 40 } }),
      500,
      { headingPageBreak: "h1" },
    );

    expect(pages).toHaveLength(1);
    expect(pageAt(pages, 0).items).toHaveLength(1);
    expect(pageAt(pages, 0).items[0]?.block.id).toBe("h1");
  });

  test("TOC-only first page is preserved instead of becoming an empty page", () => {
    const pages = paginateBlocks(
      [heading("h1")],
      measurements({ blockHeights: { h1: 40 }, tocHeight: 80 }),
      500,
      { headingPageBreak: "h1" },
    );

    expect(pages).toHaveLength(2);
    expect(pageAt(pages, 0).includeToc).toBe(true);
    expect(pageAt(pages, 0).items).toHaveLength(0);
    expect(pageAt(pages, 1).items[0]?.block.id).toBe("h1");
  });
});

describe("sanitizeCoverHtml", () => {
  test("removes scriptable elements, inline handlers and javascript URLs", () => {
    const sanitized = sanitizeCoverHtml(`
      <div class="cover" onclick="steal()">
        <h1>Safe cover</h1>
        <script>alert(1)</script>
        <img src="/logo.png" onerror="alert(2)" />
        <a href="javascript:alert(3)">link</a>
        <iframe src="https://evil.example"></iframe>
        <object data="x"></object>
        <embed src="y" />
      </div>
    `);

    expect(sanitized).toContain("Safe cover");
    expect(sanitized).toContain('class="cover"');
    expect(sanitized).not.toContain("<script");
    expect(sanitized).not.toContain("<iframe");
    expect(sanitized).not.toContain("<object");
    expect(sanitized).not.toContain("<embed");
    expect(sanitized.toLowerCase()).not.toContain("onclick");
    expect(sanitized.toLowerCase()).not.toContain("onerror");
    expect(sanitized.toLowerCase()).not.toContain("javascript:");
  });

  test("decodes obfuscated javascript protocols before dropping them", () => {
    const sanitized = sanitizeCoverHtml(
      '<p style="color:red">已知</p><a href="jav&#x61;script:alert(1)">x</a>',
    );

    expect(sanitized).toContain('style="color:red"');
    expect(sanitized).not.toContain("script:alert");
  });
});
