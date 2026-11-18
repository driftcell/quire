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

export function TableBlockView({
  table,
  layout,
  rowStart = 0,
  rowEnd,
  includeSummary = true,
}: TableBlockViewProps) {
  const leafCols = layout?.leaf_columns ?? [];
  const headerRows = layout?.header_rows ?? [];
  const allRows = Array.isArray(table.rows) ? table.rows : [];
  const visibleRows = allRows.slice(rowStart, rowEnd);
  const summaryRows =
    includeSummary && Array.isArray(table.summary_rows)
      ? table.summary_rows
      : [];

  if (leafCols.length === 0) {
    return (
      <div className="rounded border p-3 text-xs text-muted-foreground">
        表格结构尚在生成中...
      </div>
    );
  }

  return (
    <div className="w-full overflow-x-auto">
      <table className="w-full border-collapse text-sm">
        <colgroup>
          {leafCols.map((col) => (
            <col key={col.key} style={{ width: col.width_css }} />
          ))}
        </colgroup>
        <thead>
          {headerRows.map((row, rowIdx) => (
            <tr
              key={rowIdx}
              className="border-b-2 border-current/20 bg-black/[0.03] dark:bg-white/[0.04]"
            >
              {row.map((cell, cellIdx) => (
                <th
                  key={`${cell.key}-${cellIdx}`}
                  colSpan={cell.colspan}
                  rowSpan={cell.rowspan}
                  style={{ textAlign: cell.align }}
                  className="border border-black/10 px-2.5 py-2 font-semibold dark:border-white/10"
                >
                  {cell.label}
                </th>
              ))}
            </tr>
          ))}
        </thead>
        <tbody>
          {visibleRows.map((row, rIdx) => {
            const rowObj = Array.isArray(row)
              ? Object.fromEntries(
                  leafCols.map((col, idx) => [col.key, row[idx] ?? null]),
                )
              : isCellRecord(row)
                ? row
                : {};
            return (
              <tr
                key={rowStart + rIdx}
                className="border-b border-black/10 even:bg-black/[0.015] dark:border-white/10 dark:even:bg-white/[0.02]"
              >
                {leafCols.map((col) => {
                  const formatted = formatCell(rowObj[col.key]);
                  return (
                    <td
                      key={col.key}
                      title={formatted.tooltip}
                      style={{ textAlign: formatted.align ?? col.align }}
                      className={`border border-black/10 px-2.5 py-1.5 tabular-nums dark:border-white/10 ${formatted.className ?? ""}`}
                    >
                      {col.data_type === "badge" && formatted.text ? (
                        <span className="inline-block rounded-full border px-2 py-0.5 text-xs font-medium">
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
          <tfoot>
            {summaryRows.map((row, sIdx) => {
              const rowObj = Array.isArray(row)
                ? Object.fromEntries(
                    leafCols.map((col, idx) => [col.key, row[idx] ?? null]),
                  )
                : isCellRecord(row)
                  ? row
                  : {};
              return (
                <tr
                  key={sIdx}
                  className="border-t-2 border-current/25 bg-black/[0.04] font-semibold dark:bg-white/[0.06]"
                >
                  {leafCols.map((col) => {
                    const formatted = formatCell(rowObj[col.key]);
                    return (
                      <td
                        key={col.key}
                        style={{ textAlign: formatted.align ?? col.align }}
                        className="border border-black/10 px-2.5 py-1.5 tabular-nums dark:border-white/10"
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
}
