import { describe, expect, it } from "bun:test";
import { createElement } from "react";
import { renderToStaticMarkup } from "react-dom/server";
import { StyleThumbnailCard } from "@/components/style/StyleThumbnailCard";
import {
  BUILTIN_STYLE_PRESETS,
  SPECIMEN_DOC_ID,
  buildSpecimenDocument,
} from "./stylePresets";

describe("stylePresets", () => {
  it("defines 5 built-in spectrum style presets from exploratory to formal", () => {
    expect(BUILTIN_STYLE_PRESETS).toHaveLength(5);
    const ids = BUILTIN_STYLE_PRESETS.map((p) => p.style_id);
    expect(ids).toEqual([
      "field-notes",
      "modern-brief",
      "quire-folio",
      "academic-press",
      "crimson-decree",
    ]);

    for (const preset of BUILTIN_STYLE_PRESETS) {
      expect(preset.is_builtin).toBe(true);
      expect(preset.is_public).toBe(true);
      expect(preset.bundle.theme.preset).toBe(preset.style_id);
      expect(preset.bundle.theme.tokens?.["--rpt-accent"]).toMatch(
        /^#[0-9A-Fa-f]{6}$/,
      );
      expect(preset.bundle.theme.tokens?.["--rpt-paper"]).toMatch(
        /^#[0-9A-Fa-f]{6}$/,
      );
      expect(preset.bundle.theme.tokens?.["--rpt-chart-1"]).toMatch(
        /^#[0-9A-Fa-f]{6}$/,
      );
    }
  });

  it("builds a full-coverage specimen document exercising all report blocks and citation styles", () => {
    const academic = BUILTIN_STYLE_PRESETS.find(
      (p) => p.style_id === "academic-press",
    );
    if (!academic) {
      throw new Error("Missing academic-press built-in preset");
    }
    const { document, computed, pageChrome } = buildSpecimenDocument(
      academic.bundle,
    );

    expect(document.doc_id).toBe(SPECIMEN_DOC_ID);
    expect(document.theme.preset).toBe("academic-press");
    expect(document.references.style).toBe("author_year");
    expect(document.cover.enabled).toBe(true);
    expect(document.cover.variant).toBe("classic");
    expect(document.toc.enabled).toBe(true);
    expect(pageChrome.heading_page_break).toBe("h1");
    expect(computed.bibliography.length).toBe(4);
    expect(computed.citation_map.moe_2025?.label).toBe("(教育部等, 2025)");

    const types = new Set(document.blocks.map((b) => b.type));
    expect(types.has("heading")).toBe(true);
    expect(types.has("paragraph")).toBe(true);
    expect(types.has("quote")).toBe(true);
    expect(types.has("table")).toBe(true);
    expect(types.has("echarts")).toBe(true);
    expect(types.has("custom_html")).toBe(true);

    const callout = document.blocks.find((b) => b.id === "callout-summary");
    expect(callout?.type).toBe("paragraph");
    if (callout?.type === "paragraph") {
      expect(callout.style?.variant).toBe("callout");
    }
  });

  it("renders StyleThumbnailCard miniature A4 preview with preset metadata", () => {
    const crimson = BUILTIN_STYLE_PRESETS.find(
      (p) => p.style_id === "crimson-decree",
    );
    if (!crimson) {
      throw new Error("Missing crimson-decree built-in preset");
    }
    const html = renderToStaticMarkup(
      createElement(StyleThumbnailCard, {
        preset: crimson,
        selected: true,
        isDefault: false,
        isMine: false,
        onSelect: () => {},
      }),
    );
    expect(html).toContain("丹宸公文");
    expect(html).toContain("正式");
    expect(html).toContain("#991B1B");
  });
});
