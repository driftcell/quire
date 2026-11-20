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
});
