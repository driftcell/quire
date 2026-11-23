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

/**
 * Row labels in column 0 that explicitly declare an integer count unit
 * (e.g. `高层次人才总量（人）`, `国家自然科学基金年均立项（项）`, `现代产业学院建成数（个）`).
 * When a row-oriented multi-indicator table mixes these integer count rows
 * with ratio/percentage/score rows in the same numeric value columns, padding
 * integer count rows to `14.00` or `25.0` is semantically wrong.
 */
const EXPLICIT_INTEGER_ROW_UNIT_RE =
  /[（(]\s*(?:人|个|项|门|席|位|所|家|篇|部|本|次|条|套|台|名|届)\s*[）)]/;

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

function cellText(value: CellValue | undefined): string {
  if (value === undefined || value === null) {
    return "";
  }
  if (typeof value === "string") {
    return value;
  }
  if (typeof value === "number" || typeof value === "boolean") {
    return String(value);
  }
  if (isRichCell(value)) {
    return (
      value.text ??
      (typeof value.number === "number" ? String(value.number) : "")
    );
  }
  return "";
}

function rowFirstColumnText(
  row: TableRow,
  firstColKey: string | undefined,
): string {
  if (Array.isArray(row)) {
    return cellText(row[0]);
  }
  if (firstColKey && isCellRecord(row)) {
    return cellText(row[firstColKey]);
  }
  return "";
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
 *
 * If the table is a row-oriented multi-indicator table whose first column
 * explicitly labels some rows with integer count units (`（人）`, `（个）`, `（项）`, etc.)
 * and other rows with decimal values, keep `maxDecimals = 0` so integer count
 * rows are never padded with `.0` or `.00`.
 */
export function computeColumnDecimals(
  rows: TableRow[] | undefined,
  summaryRows: TableRow[] | undefined,
  leafCols: LayoutLeafColumn[],
): Map<string, number> {
  const precision = new Map<string, number>();
  const scanRows = [...(rows ?? []), ...(summaryRows ?? [])];
  const firstColKey = leafCols[0]?.key;
  const hasExplicitIntegerUnitRow =
    leafCols.length > 1 &&
    scanRows.some((row) =>
      EXPLICIT_INTEGER_ROW_UNIT_RE.test(rowFirstColumnText(row, firstColKey)),
    );

  leafCols.forEach((col, colIdx) => {
    if (!DECIMAL_ALIGN_TYPES.has(col.data_type)) {
      return;
    }
    if (colIdx > 0 && hasExplicitIntegerUnitRow) {
      precision.set(col.key, 0);
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
