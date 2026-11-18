import { memo, useMemo } from "react";
import { computeTableColumnWidths } from "@/components/report/tableSizing";
import type {
  Align,
  CellValue,
  RichCell,
  TableBlock,
  TableLayout,
} from "@/contracts/report-protocol";

export interface TableBlockViewProps {
  table: TableBlock;
  layout?: TableLayout;
  rowStart?: number;
  rowEnd?: number;
  includeSummary?: boolean;
  containerWidthPx?: number;
}

function isRichCell(value: CellValue): value is RichCell {
  return typeof value === "object" && value !== null;
}

function isCellRecord(value: unknown): value is Record<string, CellValue> {
  return typeof value === "object" && value !== null && !Array.isArray(value);
}

function formatCell(value: CellValue | undefined): {
  text: string;
  align?: Align;
  className?: string;
  tooltip?: string;
} {
  if (value === undefined || value === null) {
    return { text: "" };
  }
  if (typeof value === "string") {
    return { text: value };
  }
  if (typeof value === "number" || typeof value === "boolean") {
    return { text: String(value) };
  }
  if (isRichCell(value)) {
    return {
      text:
        value.text ?? (value.number !== undefined ? String(value.number) : ""),
      align: value.align ?? undefined,
      className: value.class_name ?? undefined,
      tooltip: value.tooltip ?? undefined,
    };
  }
  return { text: "" };
}

export const TableBlockView = memo(function TableBlockView({
  table,
  layout,
  rowStart = 0,
  rowEnd,
  includeSummary = true,
  containerWidthPx = 682,
}: TableBlockViewProps) {
  const leafCols = layout?.leaf_columns ?? [];
  const headerRows = layout?.header_rows ?? [];
  const allRows = Array.isArray(table.rows) ? table.rows : [];
  const visibleRows = allRows.slice(rowStart, rowEnd);
  const summaryRows =
    includeSummary && Array.isArray(table.summary_rows)
      ? table.summary_rows
      : [];

  const sizing = useMemo(() => {
    if (!layout || leafCols.length === 0) {
      return { columnCssWidths: [] };
    }
    return computeTableColumnWidths(table, layout, containerWidthPx);
  }, [table, layout, leafCols.length, containerWidthPx]);

  if (leafCols.length === 0) {
    return (
      <div className="rounded border p-3 text-xs text-muted-foreground">
        表格结构尚在生成中...
      </div>
    );
  }

  return (
    <div className="w-full overflow-x-auto">
      <table
        style={
          sizing.minTableWidthPx
            ? { minWidth: `${sizing.minTableWidthPx}px` }
            : undefined
        }
        className="w-full table-fixed border-collapse border-t-2 border-neutral-800 text-sm dark:border-neutral-200"
      >
        <colgroup>
          {leafCols.map((col, idx) => (
            <col
              key={col.key}
              style={{
                width: sizing.columnCssWidths[idx] ?? col.width_css,
              }}
            />
          ))}
        </colgroup>
        <thead data-measure-thead="true">
          {headerRows.map((row, rowIdx) => (
            <tr
              key={rowIdx}
              className={
                rowIdx === headerRows.length - 1
                  ? "border-b border-neutral-700 bg-[var(--rpt-surface)] dark:border-neutral-300"
                  : "border-b border-black/15 bg-[var(--rpt-surface)]"
              }
            >
              {row.map((cell, cellIdx) => (
                <th
                  key={`${cell.key}-${cellIdx}`}
                  colSpan={cell.colspan}
                  rowSpan={cell.rowspan}
                  style={{ textAlign: cell.align }}
                  className="px-3 py-2 text-[13px] font-semibold break-words"
                >
                  {cell.label}
                </th>
              ))}
            </tr>
          ))}
        </thead>
        <tbody>
          {visibleRows.map((row, rIdx) => {
            const rowObj: Record<string, CellValue> = Array.isArray(row)
              ? Object.fromEntries(
                  leafCols.map((col, idx) => [col.key, row[idx] ?? null]),
                )
              : isCellRecord(row)
                ? row
                : {};
            return (
              <tr
                key={rowStart + rIdx}
                data-measure-row-index={rowStart + rIdx}
                className="border-b border-black/[0.06] even:bg-black/[0.012] dark:border-white/[0.08] dark:even:bg-white/[0.015]"
              >
                {leafCols.map((col) => {
                  const formatted = formatCell(rowObj[col.key]);
                  return (
                    <td
                      key={col.key}
                      title={formatted.tooltip}
                      style={{ textAlign: formatted.align ?? col.align }}
                      className={`px-3 py-2 text-[13px] leading-5 break-words tabular-nums ${formatted.className ?? ""}`}
                    >
                      {col.data_type === "badge" && formatted.text ? (
                        <span className="inline-block rounded-full border px-2 py-0.5 text-xs font-medium whitespace-nowrap">
                          {formatted.text}
                        </span>
                      ) : (
                        formatted.text
                      )}
                    </td>
                  );
                })}
              </tr>
            );
          })}
        </tbody>
        {summaryRows.length > 0 && (
          <tfoot
            data-measure-tfoot="true"
            className="border-t border-neutral-700 dark:border-neutral-300"
          >
            {summaryRows.map((row, sIdx) => {
              const rowObj: Record<string, CellValue> = Array.isArray(row)
                ? Object.fromEntries(
                    leafCols.map((col, idx) => [col.key, row[idx] ?? null]),
                  )
                : isCellRecord(row)
                  ? row
                  : {};
              const isLast = sIdx === summaryRows.length - 1;
              return (
                <tr
                  key={sIdx}
                  className={`bg-[var(--rpt-surface)] font-semibold ${
                    isLast
                      ? "border-b-2 border-neutral-800 dark:border-neutral-200"
                      : ""
                  }`}
                >
                  {leafCols.map((col) => {
                    const formatted = formatCell(rowObj[col.key]);
                    return (
                      <td
                        key={col.key}
                        style={{ textAlign: formatted.align ?? col.align }}
                        className="px-3 py-2 text-[13px] leading-5 break-words tabular-nums"
                      >
                        {formatted.text}
                      </td>
                    );
                  })}
                </tr>
              );
            })}
          </tfoot>
        )}
      </table>
    </div>
  );
});
