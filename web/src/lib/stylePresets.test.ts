import { describe, expect, it } from "bun:test";
import { createElement } from "react";
import { renderToStaticMarkup } from "react-dom/server";
import { StyleCustomizerPanel } from "@/components/style/StyleCustomizerPanel";
import { StyleThumbnailCard } from "@/components/style/StyleThumbnailCard";
import {
  BUILTIN_STYLE_PRESETS,
  SPECIMEN_DOC_ID,
  buildSpecimenDocument,
} from "./stylePresets";

describe("stylePresets", () => {
  it("defines 10 built-in style presets with distinct structural heading and table architectures", () => {
    expect(BUILTIN_STYLE_PRESETS).toHaveLength(10);
    const ids = BUILTIN_STYLE_PRESETS.map((p) => p.style_id);
    expect(ids).toEqual([
      "field-notes",
      "modern-brief",
      "quire-folio",
      "academic-press",
      "crimson-decree",
      "swiss-grid",
      "blueprint-ledger",
      "monocle-magazine",
      "cyber-terminal",
      "bamboo-scroll",
    ]);

    const h1Variants = new Set(
      BUILTIN_STYLE_PRESETS.map(
        (p) => p.bundle.theme.tokens?.["--rpt-h1-variant"],
      ),
    );
    expect(h1Variants.size).toBeGreaterThanOrEqual(6);

    const h2Variants = new Set(
      BUILTIN_STYLE_PRESETS.map(
        (p) => p.bundle.theme.tokens?.["--rpt-h2-variant"],
      ),
    );
    expect(h2Variants.size).toBeGreaterThanOrEqual(5);

    const papers = new Set(
      BUILTIN_STYLE_PRESETS.map((p) => p.bundle.theme.tokens?.["--rpt-paper"]),
    );
    expect(papers.size).toBeGreaterThanOrEqual(5);

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

  it("builds a full-coverage specimen document honoring cover, TOC, and reference bundle controls", () => {
    const academic = BUILTIN_STYLE_PRESETS.find(
      (p) => p.style_id === "academic-press",
    );
    if (!academic) {
      throw new Error("Missing academic-press built-in preset");
    }
    const { document, computed, pageChrome } = buildSpecimenDocument({
      ...academic.bundle,
      toc_enabled: true,
      toc_max_depth: 2,
      toc_variant: "numbered",
      toc_page_break_after: true,
      cover_enabled: false,
    });

    expect(document.doc_id).toBe(SPECIMEN_DOC_ID);
    expect(document.theme.preset).toBe("academic-press");
    expect(document.references.style).toBe("author_year");
    expect(document.cover.enabled).toBe(false);
    expect(document.toc.enabled).toBe(true);
    expect(document.toc.max_depth).toBe(2);
    expect(document.toc.variant).toBe("numbered");
    expect(document.toc.page_break_after).toBe(true);
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
  });

  it("renders StyleThumbnailCard miniature A4 preview and 6-partition StyleCustomizerPanel", () => {
    const cyber = BUILTIN_STYLE_PRESETS.find(
      (p) => p.style_id === "cyber-terminal",
    );
    if (!cyber) {
      throw new Error("Missing cyber-terminal built-in preset");
    }
    const thumbHtml = renderToStaticMarkup(
      createElement(StyleThumbnailCard, {
        preset: cyber,
        selected: true,
        isDefault: false,
        isMine: false,
        onSelect: () => {},
      }),
    );
    expect(thumbHtml).toContain("曜石终端");
    expect(thumbHtml).toContain("暗色 · 极客");

    const panelHtml = renderToStaticMarkup(
      createElement(StyleCustomizerPanel, {
        bundle: cyber.bundle,
        onChange: () => {},
        onReset: () => {},
      }),
    );
    expect(panelHtml).toContain("色彩");
    expect(panelHtml).toContain("排版");
    expect(panelHtml).toContain("标题与块");
    expect(panelHtml).toContain("表与图");
    expect(panelHtml).toContain("封面目录");
    expect(panelHtml).toContain("分页与CSS");
    expect(panelHtml).toContain("重置修改");
    expect(panelHtml.match(/title="重置当前预设修改"/g)).toHaveLength(1);
    expect(panelHtml).toContain('aria-label="样式调优分区" class="grid w-full');
  });
});
