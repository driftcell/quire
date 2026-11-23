import { describe, expect, test } from "bun:test";
import type {
  ColumnDataType,
  LayoutLeafColumn,
} from "@/contracts/report-protocol";
import { computeColumnDecimals, formatTableCell } from "./tableCellFormat";

function leafColumn(
  index: number,
  key: string,
  label: string,
  dataType: ColumnDataType,
): LayoutLeafColumn {
  return {
    align: "left",
    data_type: dataType,
    header_align: "center",
    index,
    key,
    label,
    path: [label],
    sizing: "fit",
    weight: 1,
    width_css: "auto",
    width_pct: 0,
  };
}

describe("computeColumnDecimals", () => {
  test("uses the largest decimal count across body and summary rows", () => {
    const columns = [
      leafColumn(0, "rate", "增速", "number"),
      leafColumn(1, "label", "说明", "text"),
    ];

    const precision = computeColumnDecimals(
      [
        { rate: 45, label: "4.5" },
        { rate: 51.7, label: "5.17" },
      ],
      [{ rate: 48.25, label: "4.825" }],
      columns,
    );

    expect(precision.get("rate")).toBe(2);
    expect(precision.has("label")).toBe(false);
  });

  test("clamps numeric precision to two decimal places", () => {
    const columns = [leafColumn(0, "value", "数值", "currency")];
    const precision = computeColumnDecimals(
      [{ value: 12.3456 }],
      undefined,
      columns,
    );
    expect(precision.get("value")).toBe(2);
  });

  test("does not pad integer count rows in row-oriented multi-indicator tables", () => {
    const columns = [
      leafColumn(0, "metric", "核心指标", "text"),
      leafColumn(1, "y2025", "2025现值", "number"),
      leafColumn(2, "y2030", "2030目标", "number"),
    ];
    const rows = [
      { metric: "高层次人才总量（人）", y2025: 14, y2030: 35 },
      { metric: "领军人才省占比（%）", y2025: 0.35, y2030: 0.9 },
      { metric: "国家自然科学基金年均立项（项）", y2025: 25, y2030: 45 },
    ];
    const precision = computeColumnDecimals(rows, undefined, columns);
    expect(precision.get("y2025")).toBe(0);
    expect(precision.get("y2030")).toBe(0);
    expect(formatTableCell(14, precision.get("y2025")).text).toBe("14");
    expect(formatTableCell(0.35, precision.get("y2025")).text).toBe("0.35");
  });
});

describe("formatTableCell", () => {
  test("pads raw numbers to the column precision without touching strings", () => {
    expect(formatTableCell(45, 1).text).toBe("45.0");
    expect(formatTableCell(51.7, 1).text).toBe("51.7");
    expect(formatTableCell(45, 2).text).toBe("45.00");
    expect(formatTableCell("45.0", 2).text).toBe("45.0");
  });

  test("formats RichCell.number unless explicit text is present", () => {
    expect(formatTableCell({ number: 45 }, 1).text).toBe("45.0");
    expect(formatTableCell({ number: 45, text: "约 45" }, 1).text).toBe(
      "约 45",
    );
  });
});
