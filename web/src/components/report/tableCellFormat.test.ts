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
