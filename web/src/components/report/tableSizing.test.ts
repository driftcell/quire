import { describe, expect, test } from "bun:test";
import type {
  ColumnDataType,
  ColumnSizing,
  LayoutLeafColumn,
  TableBlock,
  TableLayout,
} from "@/contracts/report-protocol";
import {
  computeTableColumnWidths,
  type TableSizingResult,
} from "./tableSizing";

function leafColumn(
  index: number,
  key: string,
  label: string,
  dataType: ColumnDataType,
  sizing: ColumnSizing = "fit",
  weight = 1,
): LayoutLeafColumn {
  return {
    align: "left",
    data_type: dataType,
    header_align: "center",
    index,
    key,
    label,
    path: [label],
    sizing,
    weight,
    width_css: "auto",
    width_pct: 0,
  };
}

function layout(columns: LayoutLeafColumn[]): TableLayout {
  const each = 100 / columns.length;
  return {
    header_depth: 1,
    header_rows: [],
    leaf_columns: columns.map((column) => ({
      ...column,
      width_pct: each,
    })),
    table_block_id: "tbl_sizing",
    total_width_pct: 100,
  };
}

function table(rows: Array<Record<string, string | number>>): TableBlock {
  return {
    type: "table",
    columns: [],
    rows,
  };
}

function percentSum(result: TableSizingResult): number {
  return result.columnCssWidths.reduce(
    (sum, width) => sum + Number.parseFloat(width),
    0,
  );
}

function pixelWidth(
  result: TableSizingResult,
  index: number,
  container: number,
): number {
  const width = result.columnCssWidths[index];
  if (!width) {
    throw new Error(`missing column width at index ${index}`);
  }
  return (Number.parseFloat(width) / 100) * container;
}

const FALLBACK_CJK_PX = 14;
const FALLBACK_ASCII_PX = 8;
const CELL_CHROME_PX = 24;

/** Mirror the Canvas-less fallback used by Bun so assertions stay deterministic. */
function singleLineWidthPx(text: string): number {
  let units = 0;
  for (const char of text) {
    units += char.charCodeAt(0) > 127 ? FALLBACK_CJK_PX : FALLBACK_ASCII_PX;
  }
  return units + CELL_CHROME_PX;
}

function expectAtLeastSingleLine(
  result: TableSizingResult,
  index: number,
  text: string,
  container = 682,
): void {
  expect(pixelWidth(result, index, container)).toBeGreaterThanOrEqual(
    singleLineWidthPx(text) - 0.05,
  );
}

const sampleTable = table([
  {
    school: "浙江大学",
    projects: 214,
    funding: 18650,
    share: "18.4%",
    note: "省属高校第一梯队，面上项目占比仍需提升",
    rank: 1,
    growth: "12.6%",
  },
  {
    school: "浙江工业大学",
    projects: 158,
    funding: 12420,
    share: "13.6%",
    note: "青年基金增长较快，重点项目储备不足",
    rank: 2,
    growth: "9.8%",
  },
  {
    school: "宁波大学",
    projects: 132,
    funding: 9870,
    share: "11.3%",
    note: "力学与水产学科优势明显",
    rank: 3,
    growth: "7.4%",
  },
  {
    school: "杭州电子科技大学",
    projects: 119,
    funding: 8460,
    share: "10.2%",
    note: "信息学科群贡献主要增量",
    rank: 4,
    growth: "6.1%",
  },
  {
    school: "浙江师范大学",
    projects: 104,
    funding: 7120,
    share: "8.9%",
    note: "基础学科项目占比稳定",
    rank: 5,
    growth: "4.9%",
  },
]);

const sampleLayout = layout([
  leafColumn(0, "school", "学校", "text", "grow", 1.2),
  leafColumn(1, "projects", "立项数", "number"),
  leafColumn(2, "funding", "经费（万元）", "currency"),
  leafColumn(3, "share", "占比", "percent"),
  leafColumn(4, "note", "备注", "text", "grow", 1.5),
  leafColumn(5, "rank", "省内排名", "number"),
  leafColumn(6, "growth", "同比增速", "percent"),
]);

describe("computeTableColumnWidths", () => {
  test("allocates exactly 100% with default 13px metrics and a custom font family", () => {
    const defaults = computeTableColumnWidths(sampleTable, sampleLayout, 682);
    expect(defaults.columnCssWidths).toHaveLength(7);
    expect(percentSum(defaults)).toBeCloseTo(100, 1);
    expect(defaults.minTableWidthPx).toBeUndefined();

    const custom = computeTableColumnWidths(
      sampleTable,
      sampleLayout,
      682,
      '"Noto Serif SC", "Source Han Serif SC", serif',
    );
    expect(custom.columnCssWidths).toHaveLength(7);
    expect(percentSum(custom)).toBeCloseTo(100, 1);
    expect(custom.minTableWidthPx).toBeUndefined();
  });

  test("sets minTableWidthPx only for a genuinely narrow container", () => {
    const wide = computeTableColumnWidths(sampleTable, sampleLayout, 682);
    expect(wide.minTableWidthPx).toBeUndefined();
    expect(percentSum(wide)).toBeCloseTo(100, 1);

    const narrow = computeTableColumnWidths(sampleTable, sampleLayout, 360);
    expect(narrow.minTableWidthPx).toBeDefined();
    if (narrow.minTableWidthPx === undefined) {
      throw new Error(
        "expected a minimum table width for the narrow container",
      );
    }
    expect(narrow.minTableWidthPx).toBeGreaterThan(360);
    expect(percentSum(narrow)).toBeCloseTo(100, 1);
  });

  test("does not compress atomic number/percent/badge columns when space is sufficient", () => {
    const atomicTable = table([
      { projects: 214, share: "18.4%", status: "A类" },
      { projects: 158, share: "13.6%", status: "B类" },
      { projects: 132, share: "11.3%", status: "A类" },
    ]);
    const atomicLayout = layout([
      leafColumn(0, "projects", "立项数", "number"),
      leafColumn(1, "share", "占比", "percent"),
      leafColumn(2, "status", "等级", "badge"),
    ]);

    const result = computeTableColumnWidths(atomicTable, atomicLayout, 682);
    expect(result.minTableWidthPx).toBeUndefined();
    expect(percentSum(result)).toBeCloseTo(100, 1);
    for (let index = 0; index < atomicLayout.leaf_columns.length; index += 1) {
      expect(pixelWidth(result, index, 682)).toBeGreaterThanOrEqual(52);
    }
  });

  test("keeps an 8-column A4 benchmark table inside 682px without manual widths", () => {
    const benchmarkTable = table([
      {
        school: "A校（本校）",
        projects2021: 214,
        funding2021: 18650,
        rate2021: "18.4%",
        projects2020: 190,
        growth: "12.6%",
        rank: 1,
        share: "22.1%",
      },
      {
        school: "B校（标杆）",
        projects2021: 188,
        funding2021: 16200,
        rate2021: "16.9%",
        projects2020: 174,
        growth: "8.0%",
        rank: 2,
        share: "19.4%",
      },
      {
        school: "C校（同梯队）",
        projects2021: 163,
        funding2021: 14150,
        rate2021: "15.2%",
        projects2020: 151,
        growth: "7.9%",
        rank: 3,
        share: "16.8%",
      },
    ]);
    const benchmarkLayout = layout([
      leafColumn(0, "school", "高校", "text", "grow", 1.2),
      leafColumn(1, "projects2021", "2021年（项）", "number"),
      leafColumn(2, "funding2021", "经费（万元）", "currency"),
      leafColumn(3, "rate2021", "资助率（%）", "percent"),
      leafColumn(4, "projects2020", "2020年（项）", "number"),
      leafColumn(5, "growth", "同比增速（%）", "percent"),
      leafColumn(6, "rank", "省内排名", "number"),
      leafColumn(7, "share", "占全省比（%）", "percent"),
    ]);

    const result = computeTableColumnWidths(
      benchmarkTable,
      benchmarkLayout,
      682,
    );
    expect(result.columnCssWidths).toHaveLength(8);
    expect(result.minTableWidthPx).toBeUndefined();
    expect(pixelWidth(result, 0, 682)).toBeGreaterThanOrEqual(84);
    expect(percentSum(result).toFixed(2)).toBe("100.00");
  });
});

describe("computeTableColumnWidths regression boundaries", () => {
  test("keeps 7-column qualitative benchmark headers and short enums on one line", () => {
    const qualitativeTable = table([
      {
        school: "浙江海洋大学",
        region: "浙江·农林类",
        degree: "博士",
        rank: 236,
        goal: "目标双优 · 明确提出",
        positioning: "应用研究型 · 明确提出",
        candidate: "是",
      },
      {
        school: "大连海洋大学",
        region: "辽宁·农林类",
        degree: "硕士",
        rank: 296,
        goal: "目标双优 · 明确提出",
        positioning: "应用型 · 明确提出",
        candidate: "是",
      },
      {
        school: "广东海洋大学",
        region: "广东·农林类",
        degree: "博士",
        rank: 298,
        goal: "目标双一流 · 间接提出",
        positioning: "教学研究型 · 明确提出",
        candidate: "是",
      },
    ]);
    const qualitativeLayout = layout([
      leafColumn(0, "school", "高校", "text", "grow", 1.2),
      leafColumn(1, "region", "省份·类型", "text"),
      leafColumn(2, "degree", "学位层级", "text"),
      leafColumn(3, "rank", "软科排名", "number"),
      leafColumn(4, "goal", "学校目标（表述清晰度）", "text"),
      leafColumn(5, "positioning", "办学定位（表述清晰度）", "text"),
      leafColumn(6, "candidate", "双优候选池", "text"),
    ]);

    const result = computeTableColumnWidths(
      qualitativeTable,
      qualitativeLayout,
      682,
    );

    expectAtLeastSingleLine(result, 2, "学位层级");
    expectAtLeastSingleLine(result, 3, "软科排名");
    expectAtLeastSingleLine(result, 1, "浙江·农林类");
    expect(percentSum(result).toFixed(2)).toBe("100.00");
  });

  test("keeps an 8-column indicator table inside 682px without manual widths", () => {
    const indicatorTable = table([
      {
        indicator: "海洋科学",
        owner: "学科办",
        scope: "2021—2025",
        zjou: 45,
        nationalRank: 12,
        dalian: 8,
        guangdong: 9,
        jiangsu: 7,
      },
      {
        indicator: "水产",
        owner: "科研院",
        scope: "2021—2025",
        zjou: 51,
        nationalRank: 9,
        dalian: 7,
        guangdong: 10,
        jiangsu: 8,
      },
    ]);
    const indicatorLayout = layout([
      leafColumn(0, "indicator", "指标", "text"),
      leafColumn(1, "owner", "归口", "text"),
      leafColumn(2, "scope", "统计口径", "text"),
      leafColumn(3, "zjou", "浙海大", "number"),
      leafColumn(4, "nationalRank", "全国位次", "number"),
      leafColumn(5, "dalian", "大连", "number"),
      leafColumn(6, "guangdong", "广东", "number"),
      leafColumn(7, "jiangsu", "江苏", "number"),
    ]);

    const result = computeTableColumnWidths(
      indicatorTable,
      indicatorLayout,
      682,
    );

    expect(result.columnCssWidths).toHaveLength(8);
    expect(result.minTableWidthPx).toBeUndefined();
    expectAtLeastSingleLine(result, 3, "浙海大");
    expect(percentSum(result).toFixed(2)).toBe("100.00");
  });

  test("measures zero-padded decimals exactly as rendered", () => {
    const paddedTable = table([
      { small: 45, big: 123456 },
      { small: 51.7, big: 7.2 },
    ]);
    const paddedLayout = layout([
      leafColumn(0, "small", "小值", "number"),
      leafColumn(1, "big", "大值", "number"),
    ]);

    const result = computeTableColumnWidths(paddedTable, paddedLayout, 682);

    expectAtLeastSingleLine(result, 0, "45.0");
    expectAtLeastSingleLine(result, 1, "123456.0");
  });

  test("explicit percentages cannot collapse the first text column below 84px", () => {
    const narrowTable: TableBlock = {
      ...table([{ school: "深圳大学", value: 12 }]),
      style: {
        column_widths: { school: "5%", value: "95%" },
      },
    };
    const narrowLayout = layout([
      leafColumn(0, "school", "高校", "text"),
      leafColumn(1, "value", "数量", "number"),
    ]);

    const result = computeTableColumnWidths(narrowTable, narrowLayout, 682);

    expect(pixelWidth(result, 0, 682)).toBeGreaterThanOrEqual(84);
    expect(percentSum(result).toFixed(2)).toBe("100.00");
  });

  test("converts explicit px column widths against the available width", () => {
    const pxTable: TableBlock = {
      ...table([{ metric: "学科", value: 12, note: "A" }]),
      style: { column_widths: { metric: "136px" } },
    };
    const pxLayout = layout([
      leafColumn(0, "metric", "指标", "text"),
      leafColumn(1, "value", "数量", "number"),
      leafColumn(2, "note", "备注", "text"),
    ]);

    const result = computeTableColumnWidths(pxTable, pxLayout, 682);

    expect(Number.parseFloat(result.columnCssWidths[0] ?? "0")).toBeCloseTo(
      (136 / 682) * 100,
      2,
    );
    expect(percentSum(result).toFixed(2)).toBe("100.00");
  });
});
