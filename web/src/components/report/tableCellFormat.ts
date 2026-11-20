import type {
  Align,
  CellValue,
  ColumnDataType,
  LayoutLeafColumn,
  RichCell,
} from "@/contracts/report-protocol";

const DECIMAL_ALIGN_TYPES = new Set<ColumnDataType>([
  "number",
  "percent",
  "currency",
]);

type TableRow = CellValue[] | Record<string, CellValue>;

export interface FormattedTableCell {
  text: string;
  align?: Align;
  className?: string;
  tooltip?: string;
}

function isRichCell(value: CellValue): value is RichCell {
  return typeof value === "object" && value !== null;
}

function isCellRecord(value: unknown): value is Record<string, CellValue> {
  return typeof value === "object" && value !== null && !Array.isArray(value);
}

function rawNumber(value: CellValue | undefined): number | undefined {
  if (value === undefined || value === null) {
    return undefined;
  }
  if (typeof value === "number") {
    return value;
  }
  if (isRichCell(value) && typeof value.number === "number") {
    return value.number;
  }
  return undefined;
}

function decimalPlaces(value: number): number {
  if (!Number.isFinite(value)) {
    return 0;
  }
  const text = String(value);
  const dot = text.indexOf(".");
  return dot === -1 ? 0 : text.length - dot - 1;
}

function formatNumber(value: number, maxDecimals: number): string {
  if (maxDecimals > 0 && Number.isFinite(value)) {
    return value.toFixed(maxDecimals);
  }
  return String(value);
}

/**
 * Scan all body and summary rows for one column and return how many decimal
 * places are needed to keep raw numeric cells visually aligned. Explicit
 * strings are never reformatted.
 */
export function computeColumnDecimals(
  rows: TableRow[] | undefined,
  summaryRows: TableRow[] | undefined,
  leafCols: LayoutLeafColumn[],
): Map<string, number> {
  const precision = new Map<string, number>();
  const scanRows = [...(rows ?? []), ...(summaryRows ?? [])];

  leafCols.forEach((col, colIdx) => {
    if (!DECIMAL_ALIGN_TYPES.has(col.data_type)) {
      return;
    }
    let maxDecimals = 0;
    for (const row of scanRows) {
      const raw = Array.isArray(row)
        ? row[colIdx]
        : isCellRecord(row)
          ? row[col.key]
          : undefined;
      const numeric = rawNumber(raw);
      if (numeric !== undefined) {
        maxDecimals = Math.max(maxDecimals, decimalPlaces(numeric));
      }
    }
    precision.set(col.key, Math.min(2, maxDecimals));
  });

  return precision;
}

export function formatTableCell(
  value: CellValue | undefined,
  maxDecimals = 0,
): FormattedTableCell {
  if (value === undefined || value === null) {
    return { text: "" };
  }
  if (typeof value === "string") {
    return { text: value };
  }
  if (typeof value === "number") {
    return { text: formatNumber(value, maxDecimals) };
  }
  if (typeof value === "boolean") {
    return { text: String(value) };
  }
  if (isRichCell(value)) {
    return {
      text:
        value.text ??
        (typeof value.number === "number"
          ? formatNumber(value.number, maxDecimals)
          : ""),
      align: value.align ?? undefined,
      className: value.class_name ?? undefined,
      tooltip: value.tooltip ?? undefined,
    };
  }
  return { text: "" };
}
