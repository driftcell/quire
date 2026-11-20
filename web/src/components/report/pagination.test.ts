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

function measurements(
  overrides: Partial<PageMeasurements> = {},
): PageMeasurements {
  return {
    blockHeights: {},
    tableMeasurements: {},
    coverHeight: 0,
    tocHeight: 0,
    bibliographyHeight: 0,
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
});

describe("paginateBlocks references page break", () => {
  test("page_break_before=true moves references to a clean trailing page", () => {
    const pages = paginateBlocks(
      [paragraph("body")],
      measurements({
        blockHeights: { body: 100 },
        bibliographyHeight: 80,
      }),
      500,
      { referencesPageBreakBefore: true },
    );

    expect(pages).toHaveLength(2);
    expect(pageAt(pages, 0).includeBibliography).toBeUndefined();
    expect(pageAt(pages, 1).includeBibliography).toBe(true);
  });

  test("page_break_before=false shares the last page when references fit", () => {
    const pages = paginateBlocks(
      [paragraph("body")],
      measurements({
        blockHeights: { body: 100 },
        bibliographyHeight: 80,
      }),
      500,
      { referencesPageBreakBefore: false },
    );

    expect(pages).toHaveLength(1);
    expect(pageAt(pages, 0).includeBibliography).toBe(true);
  });

  test("page_break_before=false still breaks when references overflow", () => {
    const pages = paginateBlocks(
      [paragraph("body")],
      measurements({
        blockHeights: { body: 100 },
        bibliographyHeight: 450,
      }),
      500,
      { referencesPageBreakBefore: false },
    );

    expect(pages).toHaveLength(2);
    expect(pageAt(pages, 0).includeBibliography).toBeUndefined();
    expect(pageAt(pages, 1).includeBibliography).toBe(true);
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
