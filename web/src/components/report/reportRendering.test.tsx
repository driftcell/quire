import { describe, expect, test } from "bun:test";
import { renderToStaticMarkup } from "react-dom/server";
import { ReportBlockItem } from "@/components/report/ReportBlockItem";
import { ReportCover } from "@/components/report/ReportCover";
import type { PageSlice } from "@/components/report/pagination";
import { ReportTocSection } from "@/components/report/ReportTocSection";
import type {
  BibliographyEntry,
  ComputedDocumentMeta,
} from "@/contracts/report-protocol";
import type { ReportBlock } from "@/lib/document";
import { normalizeTocConfig } from "@/lib/tocConfig";

const computed: ComputedDocumentMeta = {
  bibliography: [],
  block_framing: {},
  citation_map: {
    source_a: {
      number: 1,
      label: "[1]",
      source_id: "source_a",
      title: "教育部年度报告",
    },
    source_b: {
      number: 2,
      label: "[2]",
      source_id: "source_b",
      title: "国家自然科学基金委员会",
    },
    source_c: {
      number: 3,
      label: "[3]",
      source_id: "source_c",
      title: "统计公报",
    },
    source_d: {
      number: 4,
      label: "[4]",
      source_id: "source_d",
      title: "统计年鉴",
    },
    source_e: {
      number: 5,
      label: "[5]",
      source_id: "source_e",
      title: "科研年报",
    },
  },
  table_layouts: {},
  toc_tree: [],
};

type TableReportBlock = Extract<ReportBlock, { type: "table" }>;

function tableBlock(): TableReportBlock {
  return {
    id: "visual-table",
    type: "table",
    title: "测试表",
    note: "单位：项；统计口径：2025年。",
    columns: [],
    rows: [],
    source_ids: ["source_a", "source_b", "source_c", "source_d"],
    source_note: "教育部",
  };
}

function renderTable(
  sliceOverrides: Partial<PageSlice> = {},
  blockOverrides: Partial<
    Pick<TableReportBlock, "source_ids" | "source_note">
  > = {},
  bibliographyById = new Map<string, BibliographyEntry>(),
): string {
  const block: TableReportBlock = { ...tableBlock(), ...blockOverrides };
  return renderToStaticMarkup(
    <ReportBlockItem
      slice={{ block, ...sliceOverrides }}
      isPaged={true}
      printMode={true}
      computed={computed}
      bibliographyById={bibliographyById}
      commentCount={0}
      onCitationClick={() => {}}
    />,
  );
}

function bibliographyEntry(
  sourceId: string,
  number: number,
  publisher: string,
  title: string,
): BibliographyEntry {
  return {
    source_id: sourceId,
    number,
    label: `[${number}]`,
    plain_text: title,
    source: { publisher, title },
  };
}

describe("table footer notes", () => {
  test("notesOnly renders footer notes without repeating the table or caption", () => {
    const markup = renderTable({ notesOnly: true });

    expect(markup).toContain("注：");
    expect(markup).toContain("资料来源：");
    expect(markup).not.toContain("<figcaption");
    expect(markup).not.toContain("表格结构尚在生成中");
  });

  test("includeNotes=false suppresses notes on the final body slice", () => {
    const markup = renderTable({ includeSummary: true, includeNotes: false });

    expect(markup).not.toContain("注：");
    expect(markup).not.toContain("资料来源：");
  });

  test("fills missing source notes with deduplicated bibliography names", () => {
    const bibliographyById = new Map<string, BibliographyEntry>([
      [
        "source_a",
        bibliographyEntry("source_a", 1, " 教育部 ", "教育部年度报告"),
      ],
      [
        "source_b",
        bibliographyEntry("source_b", 2, "", "国家自然科学基金委员会"),
      ],
      ["source_c", bibliographyEntry("source_c", 3, "教育部", "教育统计公报")],
      ["source_d", bibliographyEntry("source_d", 4, "国家统计局", "统计年鉴")],
      [
        "source_e",
        bibliographyEntry("source_e", 5, "高校科研年报", "科研年报"),
      ],
    ]);
    const markup = renderTable(
      {},
      {
        source_note: "  ",
        source_ids: [
          "source_a",
          "source_b",
          "source_c",
          "source_d",
          "source_e",
        ],
      },
      bibliographyById,
    );

    expect(markup).toContain("教育部、国家自然科学基金委员会、国家统计局");
    expect(markup).not.toContain("高校科研年报");
    expect(markup).toContain("[1,2,3,4,5]");
  });
});

describe("cover subtitle cleanup", () => {
  test("classic adds exactly one pair of dashes around a cleaned subtitle", () => {
    const markup = renderToStaticMarkup(
      <ReportCover
        cover={{
          enabled: true,
          variant: "classic",
          title: "测试报告",
          subtitle: "—— ——基于2025年数据―― ",
        }}
      />,
    ).replace(/<!--.*?-->/g, "");

    expect(markup).toContain("—— 基于2025年数据 ——");
    expect(markup).not.toContain("—— ——");
  });

  test.each(["modern", "minimal", "split"])(
    "%s renders the same cleaned subtitle without adding dashes",
    (variant) => {
      const markup = renderToStaticMarkup(
        <ReportCover
          cover={{
            enabled: true,
            variant,
            title: "测试报告",
            subtitle: "— 基于2025年数据 —",
          }}
        />,
      );

      expect(markup).toContain("基于2025年数据");
      expect(markup).not.toContain("— 基于2025年数据 —");
    },
  );
});

describe("TOC page-number alignment", () => {
  const nodes = [
    { block_id: "chapter-one", level: 1, number: "1", text: "核心研判" },
  ];

  test("defaults to dotted leaders in continuous and paged modes", () => {
    expect(normalizeTocConfig(null).variant).toBe("dotted");
    expect(normalizeTocConfig(null, { paged: true }).variant).toBe("dotted");
  });

  test.each(["standard", "numbered"])(
    "%s keeps page numbers pinned to the right edge",
    (variant) => {
      const config = normalizeTocConfig({ variant });
      const markup = renderToStaticMarkup(
        <ReportTocSection
          config={config}
          nodes={nodes}
          isPaged={true}
          pageMap={new Map([["chapter-one", 3]])}
          printMode={true}
        />,
      );

      expect(markup).toContain("ml-auto pl-3");
      expect(markup).toContain(">3</span>");
    },
  );
});
