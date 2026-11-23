import { memo, useLayoutEffect, useMemo, useRef, useState } from "react";
import {
  computeColumnDecimals,
  formatTableCell,
} from "@/components/report/tableCellFormat";
import { computeTableColumnWidths } from "@/components/report/tableSizing";
import { A4_CONTENT_WIDTH_PX } from "@/components/report/pagination";
import type {
  CellValue,
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

function isCellRecord(value: unknown): value is Record<string, CellValue> {
  return typeof value === "object" && value !== null && !Array.isArray(value);
}

const NUMERIC_DATA_TYPES = new Set(["number", "currency", "percent"]);
const SHORT_NUMERIC_TOKEN = /^[+-]?\d[\d,，]*(?:\.\d+)?%?$/;
const COMPOSITE_NUMERIC_TOKEN =
  /^\d+(?:\.\d+)?(?:\s*[/／·]\s*\d+(?:\.\d+)?)+%?$/;

function keepHeaderOnOneLine(label: string): boolean {
  return Array.from(label.trim()).length <= 4;
}

function keepCellOnOneLine(text: string, dataType: string): boolean {
  const trimmed = text.trim();
  if (!trimmed) {
    return false;
  }
  if (NUMERIC_DATA_TYPES.has(dataType)) {
    return true;
  }
  return (
    SHORT_NUMERIC_TOKEN.test(trimmed) || COMPOSITE_NUMERIC_TOKEN.test(trimmed)
  );
}

export const TableBlockView = memo(function TableBlockView({
  table,
  layout,
  rowStart = 0,
  rowEnd,
  includeSummary = true,
  containerWidthPx,
}: TableBlockViewProps) {
  const containerRef = useRef<HTMLDivElement>(null);
  const [measuredWidthPx, setMeasuredWidthPx] = useState<number>();
  const [computedFontFamily, setComputedFontFamily] = useState<string>();
  const leafCols = useMemo(() => layout?.leaf_columns ?? [], [layout]);
  const headerRows = layout?.header_rows ?? [];
  const allRows = Array.isArray(table.rows) ? table.rows : [];
  const visibleRows = allRows.slice(rowStart, rowEnd);
  const allSummaryRows = Array.isArray(table.summary_rows)
    ? table.summary_rows
    : [];
  const summaryRows = includeSummary ? allSummaryRows : [];

  const decimalPrecision = useMemo(
    () => computeColumnDecimals(table.rows, table.summary_rows, leafCols),
    [table.rows, table.summary_rows, leafCols],
  );

  useLayoutEffect(() => {
    const el = containerRef.current;
    if (!el) {
      return undefined;
    }
    const family = window.getComputedStyle(el).fontFamily;
    setComputedFontFamily((prev) => (prev === family ? prev : family));

    if (containerWidthPx !== undefined) {
      return undefined;
    }

    const updateWidth = () => {
      const next = el.clientWidth;
      if (next > 0) {
        setMeasuredWidthPx((prev) => (prev === next ? prev : next));
      }
    };
    updateWidth();

    if (typeof ResizeObserver === "undefined") {
      return undefined;
    }
    const observer = new ResizeObserver(updateWidth);
    observer.observe(el);
    return () => observer.disconnect();
  }, [containerWidthPx]);

  const effectiveWidthPx =
    containerWidthPx ?? measuredWidthPx ?? A4_CONTENT_WIDTH_PX;

  const sizing = useMemo(() => {
    if (!layout || leafCols.length === 0) {
      return { columnCssWidths: [] };
    }
    return computeTableColumnWidths(
      table,
      layout,
      effectiveWidthPx,
      computedFontFamily,
    );
  }, [table, layout, leafCols.length, effectiveWidthPx, computedFontFamily]);

  if (leafCols.length === 0) {
    return (
      <div className="rounded border p-3 text-xs text-muted-foreground">
        表格结构尚在生成中...
      </div>
    );
  }

  const cellPaddingClass = leafCols.length >= 6 ? "px-2.5 py-2" : "px-3 py-2";

  return (
    <div ref={containerRef} className="w-full overflow-x-auto">
      <table
        style={
          sizing.minTableWidthPx
            ? { minWidth: `${sizing.minTableWidthPx}px` }
            : undefined
        }
        className="w-full table-fixed border-collapse border-t-[1.5px] border-b-[1.5px] border-slate-800 text-sm"
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
                  ? "border-b border-slate-700 bg-slate-100/80 text-slate-800"
                  : "border-b border-slate-300/80 bg-slate-100/80 text-slate-800"
              }
            >
              {row.map((cell, cellIdx) => (
                <th
                  key={`${cell.key}-${cellIdx}`}
                  colSpan={cell.colspan}
                  rowSpan={cell.rowspan}
                  style={{ textAlign: cell.align }}
                  className={`${cellPaddingClass} text-[13px] font-semibold ${keepHeaderOnOneLine(cell.label) ? "whitespace-nowrap" : "break-words [text-wrap:balance]"}`}
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
                className="border-b border-slate-200/70 even:bg-slate-50/70"
              >
                {leafCols.map((col) => {
                  const formatted = formatTableCell(
                    rowObj[col.key],
                    decimalPrecision.get(col.key) ?? 0,
                  );
                  const keepTogether = keepCellOnOneLine(
                    formatted.text,
                    col.data_type,
                  );
                  return (
                    <td
                      key={col.key}
                      title={formatted.tooltip}
                      style={{ textAlign: formatted.align ?? col.align }}
                      className={`${cellPaddingClass} text-[13px] leading-5 tabular-nums text-slate-700 ${keepTogether ? "whitespace-nowrap" : "break-words"} ${formatted.className ?? ""}`}
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
            className="border-t border-slate-700 bg-slate-100/70 font-semibold text-slate-900"
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
                  className={`bg-slate-100/70 font-semibold ${
                    isLast ? "border-b-[1.5px] border-slate-800" : ""
                  }`}
                >
                  {leafCols.map((col) => {
                    const formatted = formatTableCell(
                      rowObj[col.key],
                      decimalPrecision.get(col.key) ?? 0,
                    );
                    const keepTogether = keepCellOnOneLine(
                      formatted.text,
                      col.data_type,
                    );
                    return (
                      <td
                        key={col.key}
                        style={{ textAlign: formatted.align ?? col.align }}
                        className={`${cellPaddingClass} text-[13px] leading-5 tabular-nums text-slate-900 ${keepTogether ? "whitespace-nowrap" : "break-words"}`}
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
