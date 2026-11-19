import { describe, expect, test } from "bun:test";
import type { ReportBlock } from "@/lib/document";
import {
  buildBlockPageMap,
  paginateBlocks,
  type A4Page,
  type PageMeasurements,
} from "./pagination";

function heading(id: string): ReportBlock {
  return { id, type: "heading", level: 1, text: id };
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
