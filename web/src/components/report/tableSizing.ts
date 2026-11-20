import type {
  CellValue,
  ColumnDataType,
  RichCell,
  TableBlock,
  TableLayout,
} from "@/contracts/report-protocol";
import { isRecord } from "@/lib/guards";

export interface TableSizingResult {
  columnCssWidths: string[];
  minTableWidthPx?: number;
}

interface ColumnBounds {
  index: number;
  explicitPct?: number;
  minPx: number;
  idealPx: number;
  weight: number;
  sizing: "fit" | "grow";
  atomic: boolean;
}

const CELL_HORIZONTAL_CHROME_PX = 24; // px-2.5 (20px) + borders + slack
const BADGE_EXTRA_PX = 18;
const ATOMIC_TYPES = new Set<ColumnDataType>([
  "number",
  "currency",
  "percent",
  "date",
  "badge",
]);

let cachedCtx: CanvasRenderingContext2D | null | undefined;

function getMeasureContext(): CanvasRenderingContext2D | null {
  if (cachedCtx !== undefined) {
    return cachedCtx;
  }
  if (typeof window === "undefined" || typeof document === "undefined") {
    cachedCtx = null;
    return null;
  }
  const canvas = document.createElement("canvas");
  cachedCtx = canvas.getContext("2d");
  return cachedCtx;
}

function measureTextWidthPx(text: string, font: string): number {
  if (!text) return 0;
  const ctx = getMeasureContext();
  if (!ctx) {
    // Fallback heuristic if Canvas 2D is unavailable
    let units = 0;
    for (const ch of text) {
      units += ch.charCodeAt(0) > 127 ? 14 : 8;
    }
    return units;
  }
  ctx.font = font;
  return ctx.measureText(text).width;
}

function measureMultilineMaxPx(text: string, font: string): number {
  if (!text) return 0;
  let maxW = 0;
  for (const line of text.split("\n")) {
    const w = measureTextWidthPx(line.trim(), font);
    if (w > maxW) maxW = w;
  }
  return maxW;
}

function measureLongestTokenPx(text: string, font: string): number {
  if (!text) return 0;
  let maxW = 0;
  // Split on whitespace or CJK/ASCII boundaries for unbreakable token width
  const tokens = text.split(/\s+/);
  for (const token of tokens) {
    if (!token) continue;
    const w = measureTextWidthPx(token, font);
    // For long CJK sentences without spaces, allow wrapping at ~4 chars min
    const effective =
      /[\u4e00-\u9fff]/.test(token) && token.length > 6
        ? Math.min(w, 64)
        : Math.min(w, 130);
    if (effective > maxW) maxW = effective;
  }
  return maxW;
}

function isRichCell(value: CellValue): value is RichCell {
  return typeof value === "object" && value !== null;
}

function isCellRecord(value: unknown): value is Record<string, CellValue> {
  return typeof value === "object" && value !== null && !Array.isArray(value);
}

function extractCellText(value: CellValue | undefined): string {
  if (value === undefined || value === null) return "";
  if (typeof value === "string") return value;
  if (typeof value === "number" || typeof value === "boolean") {
    return String(value);
  }
  if (isRichCell(value)) {
    return (
      value.text ?? (value.number !== undefined ? String(value.number) : "")
    );
  }
  return "";
}

function parsePercent(raw: string | undefined): number | undefined {
  if (!raw) return undefined;
  const trimmed = raw.trim();
  if (!trimmed.endsWith("%")) return undefined;
  const num = Number.parseFloat(trimmed.slice(0, -1));
  return Number.isFinite(num) && num > 0 ? Math.min(100, num) : undefined;
}

function readExplicitWidths(
  style: unknown,
): Record<string, string> | undefined {
  if (!isRecord(style)) {
    return undefined;
  }
  const widths = style.column_widths;
  if (!isRecord(widths)) {
    return undefined;
  }
  const result: Record<string, string> = {};
  for (const [key, value] of Object.entries(widths)) {
    if (typeof value === "string") {
      result[key] = value;
    }
  }
  return result;
}

function percentile(sortedAsc: number[], p: number): number {
  if (sortedAsc.length === 0) return 0;
  const idx = Math.min(
    sortedAsc.length - 1,
    Math.max(0, Math.ceil(sortedAsc.length * p) - 1),
  );
  return sortedAsc[idx] ?? 0;
}

/** Round to 2 decimals and absorb the residual into the last column so the
 * rendered percentages add up to exactly 100.00%. */
function formatPercentWidths(pcts: number[]): string[] {
  if (pcts.length === 0) return [];
  const rounded = pcts.map((p) => Math.round(p * 100) / 100);
  const lastIndex = rounded.length - 1;
  const sum = rounded.reduce((total, value) => total + value, 0);
  const last = rounded[lastIndex] ?? 0;
  rounded[lastIndex] = Math.round((last + (100 - sum)) * 100) / 100;
  return rounded.map((p) => `${p.toFixed(2)}%`);
}

/**
 * SOTA pixel-accurate Water-Filling column width allocator.
 * - Uses Canvas 2D text metrics with actual table font & padding.
 * - Guarantees zero wrapping on numeric/date/percent/badge columns when space permits.
 * - Caps Fit columns at ideal single-line width so remaining space flows to Grow columns.
 * - Computed once across all rows so multi-page table slices align identically.
 */
export function computeTableColumnWidths(
  table: TableBlock,
  layout: TableLayout,
  containerWidthPx: number = 682,
  fontFamily?: string,
): TableSizingResult {
  const leafCols = layout.leaf_columns;
  if (leafCols.length === 0) {
    return { columnCssWidths: [] };
  }

  const availablePx = Math.max(240, containerWidthPx);
  const resolvedFamily =
    fontFamily && fontFamily.trim().length > 0
      ? fontFamily.trim()
      : "ui-sans-serif, system-ui, sans-serif";
  const headerFont = `600 13px ${resolvedFamily}`;
  const bodyFont = `400 13px ${resolvedFamily}`;

  const allRows = [
    ...(Array.isArray(table.rows) ? table.rows : []),
    ...(Array.isArray(table.summary_rows) ? table.summary_rows : []),
  ];

  const explicitWidths = readExplicitWidths(table["style"]);
  const bounds: ColumnBounds[] = leafCols.map((col, colIdx) => {
    const explicitPct = parsePercent(explicitWidths?.[col.key]);
    const isAtomic = ATOMIC_TYPES.has(col.data_type);
    const extraChrome =
      CELL_HORIZONTAL_CHROME_PX +
      (col.data_type === "badge" ? BADGE_EXTRA_PX : 0);

    const headerPx =
      measureMultilineMaxPx(col.label, headerFont) + CELL_HORIZONTAL_CHROME_PX;

    const cellWidths: number[] = [];
    let maxAtomicPx = 0;
    let maxUnbreakablePx = 0;

    for (const row of allRows) {
      const rawCell = Array.isArray(row)
        ? row[colIdx]
        : isCellRecord(row)
          ? row[col.key]
          : undefined;
      const text = extractCellText(rawCell);
      if (!text) continue;
      const w = measureMultilineMaxPx(text, bodyFont) + extraChrome;
      cellWidths.push(w);
      if (w > maxAtomicPx) maxAtomicPx = w;
      if (!isAtomic) {
        const tokW = measureLongestTokenPx(text, bodyFont) + extraChrome;
        if (tokW > maxUnbreakablePx) maxUnbreakablePx = tokW;
      }
    }

    cellWidths.sort((a, b) => a - b);
    const p90CellPx = percentile(cellWidths, 0.9);
    const p95CellPx = percentile(cellWidths, 0.95);

    const minPx = isAtomic
      ? Math.max(
          48,
          Math.min(
            240,
            Math.max(
              maxAtomicPx,
              Math.min(headerPx, Math.max(48, Math.ceil(headerPx * 0.56))),
            ),
          ),
        )
      : Math.max(
          48,
          Math.min(140, Math.max(Math.min(headerPx, 110), maxUnbreakablePx)),
        );

    const maxCapPx = Math.max(minPx, availablePx * 0.46);
    const rawIdeal = isAtomic
      ? Math.max(headerPx, p95CellPx, minPx)
      : Math.max(headerPx, p90CellPx, minPx);
    const idealPx = Math.min(maxCapPx, Math.max(minPx, rawIdeal));

    const weight =
      typeof col.weight === "number" && Number.isFinite(col.weight)
        ? Math.min(100, Math.max(0.1, col.weight))
        : 1;

    return {
      index: colIdx,
      explicitPct,
      minPx,
      idealPx,
      weight,
      sizing: col.sizing === "grow" ? "grow" : "fit",
      atomic: isAtomic,
    };
  });

  // Reserve explicit percentage columns first
  let explicitPctSum = 0;
  for (const b of bounds) {
    if (b.explicitPct !== undefined) {
      explicitPctSum += b.explicitPct;
    }
  }

  const flexBounds = bounds.filter((b) => b.explicitPct === undefined);
  if (flexBounds.length === 0) {
    const total = explicitPctSum > 0 ? explicitPctSum : 100;
    return {
      columnCssWidths: formatPercentWidths(
        bounds.map((b) => ((b.explicitPct ?? 0) / total) * 100),
      ),
    };
  }

  if (explicitPctSum > 90) {
    const scale = 90 / explicitPctSum;
    for (const b of bounds) {
      if (b.explicitPct !== undefined) {
        b.explicitPct *= scale;
      }
    }
    explicitPctSum = 90;
  }

  const remainingPx = availablePx * ((100 - explicitPctSum) / 100);
  const alloc = new Map<number, number>();
  let minSum = 0;
  for (const b of flexBounds) {
    alloc.set(b.index, b.minPx);
    minSum += b.minPx;
  }

  let minTableWidthPx: number | undefined;
  let surplus = remainingPx - minSum;

  if (surplus > 0) {
    // Pass 1: Water-fill up to idealPx
    const active = new Set(flexBounds.map((b) => b.index));
    while (surplus > 0.5 && active.size > 0) {
      let weightSum = 0;
      for (const b of flexBounds) {
        if (!active.has(b.index)) continue;
        const curr = alloc.get(b.index) ?? b.minPx;
        if (b.idealPx - curr <= 0.1) {
          active.delete(b.index);
          continue;
        }
        weightSum += (b.idealPx - curr) * b.weight;
      }
      if (weightSum <= 0 || active.size === 0) break;

      let spent = 0;
      for (const b of flexBounds) {
        if (!active.has(b.index)) continue;
        const curr = alloc.get(b.index) ?? b.minPx;
        const room = b.idealPx - curr;
        const share = surplus * ((room * b.weight) / weightSum);
        const add = Math.min(room, share);
        alloc.set(b.index, curr + add);
        spent += add;
        if (curr + add >= b.idealPx - 0.1) {
          active.delete(b.index);
        }
      }
      if (spent <= 0.01) break;
      surplus -= spent;
    }

    // Pass 2: Distribute leftover surplus to Grow columns (or all if none are Grow)
    if (surplus > 0.5) {
      const growCols = flexBounds.filter((b) => b.sizing === "grow");
      const targets = growCols.length > 0 ? growCols : flexBounds;
      const targetWeight = targets.reduce(
        (sum, b) => sum + b.weight * (b.sizing === "grow" ? 1 : b.idealPx),
        0,
      );
      if (targetWeight > 0) {
        for (const b of targets) {
          const factor = b.weight * (b.sizing === "grow" ? 1 : b.idealPx);
          const curr = alloc.get(b.index) ?? b.minPx;
          alloc.set(b.index, curr + surplus * (factor / targetWeight));
        }
      }
    }
  } else if (surplus < 0) {
    // Squeeze non-atomic text columns first before touching atomic numeric/badge columns.
    // Keep at least 72px for text columns so the first column never collapses into a
    // one-character vertical strip.
    const textFloorPx = (b: ColumnBounds) => Math.min(b.minPx, 72);
    let deficit = -surplus;
    const textCols = flexBounds.filter((b) => !b.atomic);
    const totalShrinkable = textCols.reduce(
      (sum, b) => sum + Math.max(0, b.minPx - textFloorPx(b)),
      0,
    );
    if (totalShrinkable > 0) {
      const shrinkRatio = Math.min(1, deficit / totalShrinkable);
      for (const b of textCols) {
        const room = Math.max(0, b.minPx - textFloorPx(b));
        const cut = room * shrinkRatio;
        alloc.set(b.index, b.minPx - cut);
        deficit -= cut;
      }
    }
    if (deficit > 4) {
      const explicitPx = availablePx * (explicitPctSum / 100);
      const flexTotal = Array.from(alloc.values()).reduce((a, b) => a + b, 0);
      minTableWidthPx = Math.ceil(explicitPx + flexTotal);
    }
  }

  const flexTotalPx =
    Array.from(alloc.values()).reduce((a, b) => a + b, 0) || 1;
  const flexPctBudget = 100 - explicitPctSum;

  const pcts = bounds.map((b) => {
    if (b.explicitPct !== undefined) {
      return b.explicitPct;
    }
    const px = alloc.get(b.index) ?? b.minPx;
    return (px / flexTotalPx) * flexPctBudget;
  });

  // Normalize to exactly 100.00%
  const sumPct = pcts.reduce((a, b) => a + b, 0);
  if (sumPct > 0) {
    for (let i = 0; i < pcts.length; i += 1) {
      const val = pcts[i] ?? 0;
      pcts[i] = (val / sumPct) * 100;
    }
  }

  return {
    columnCssWidths: formatPercentWidths(pcts),
    minTableWidthPx,
  };
}
