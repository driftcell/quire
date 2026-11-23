import type {
  CellValue,
  ColumnDataType,
  TableBlock,
  TableLayout,
} from "@/contracts/report-protocol";
import { computeColumnDecimals, formatTableCell } from "./tableCellFormat";
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
  maxCapPx: number;
  weight: number;
  sizing: "fit" | "grow";
  atomic: boolean;
}

const CELL_HORIZONTAL_PADDING_PX = 24;
const CELL_HORIZONTAL_CHROME_PX = 30; // px-3 (24px) + borders + rendering slack
const ATOMIC_SAFETY_RECLAIM_PX =
  CELL_HORIZONTAL_CHROME_PX - CELL_HORIZONTAL_PADDING_PX;
const BADGE_EXTRA_PX = 18;
const SHORT_HEADER_SINGLE_LINE_PX = 92;
const WRAPPED_HEADER_FLOOR_PX = 78;
const COMPACT_NUMERIC_WRAPPED_HEADER_FLOOR_PX = 68;
const TEXT_COMPRESSION_FLOOR_PX = 78;
const FIRST_TEXT_COLUMN_FLOOR_PX = 84;
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
    // Long CJK sentences may wrap at ~4 chars; short words (<= 7 chars)
    // keep their full single-line width so labels like "浙江·农林类" never
    // lose a character to an orphan line.
    const effective = Math.min(
      w,
      /[\u4e00-\u9fff]/.test(token) && token.length > 7 ? 64 : 110,
    );
    if (effective > maxW) maxW = effective;
  }
  return maxW;
}

function isCellRecord(value: unknown): value is Record<string, CellValue> {
  return typeof value === "object" && value !== null && !Array.isArray(value);
}

function parseExplicitWidthPct(
  raw: string | undefined,
  availablePx: number,
): number | undefined {
  if (!raw) return undefined;
  const trimmed = raw.trim();
  const lower = trimmed.toLowerCase();
  let pct: number;
  if (lower.endsWith("%")) {
    pct = Number.parseFloat(trimmed.slice(0, -1));
  } else if (lower.endsWith("px")) {
    const px = Number.parseFloat(trimmed.slice(0, -2));
    pct = (px / availablePx) * 100;
  } else {
    return undefined;
  }
  return Number.isFinite(pct) && pct > 0 ? Math.min(100, pct) : undefined;
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

function compressionFloorPx(bounds: ColumnBounds): number {
  return bounds.index === 0 && !bounds.atomic
    ? FIRST_TEXT_COLUMN_FLOOR_PX
    : TEXT_COMPRESSION_FLOOR_PX;
}

function explicitFloorPct(bounds: ColumnBounds, availablePx: number): number {
  const floorPx = Math.min(bounds.minPx, compressionFloorPx(bounds));
  return (floorPx / availablePx) * 100;
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
  const columnDecimals = computeColumnDecimals(allRows, undefined, leafCols);
  const bounds: ColumnBounds[] = leafCols.map((col, colIdx) => {
    const explicitPct = parseExplicitWidthPct(
      explicitWidths?.[col.key],
      availablePx,
    );
    const isAtomic = ATOMIC_TYPES.has(col.data_type);
    const extraChrome =
      CELL_HORIZONTAL_CHROME_PX +
      (col.data_type === "badge" ? BADGE_EXTRA_PX : 0);

    const headerPx =
      measureMultilineMaxPx(col.label, headerFont) + CELL_HORIZONTAL_CHROME_PX;

    const decimals = columnDecimals.get(col.key) ?? 0;
    const cellWidths: number[] = [];
    let maxAtomicPx = 0;
    let maxUnbreakablePx = 0;

    for (const row of allRows) {
      const rawCell = Array.isArray(row)
        ? row[colIdx]
        : isCellRecord(row)
          ? row[col.key]
          : undefined;
      const text = formatTableCell(rawCell, decimals).text;
      if (!text) continue;
      const w = measureMultilineMaxPx(text, bodyFont) + extraChrome;
      cellWidths.push(w);
      if (w > maxAtomicPx) maxAtomicPx = w;
      if (!isAtomic) {
        const tokW = measureLongestTokenPx(text, bodyFont) + extraChrome;
        if (tokW > maxUnbreakablePx) maxUnbreakablePx = tokW;
      }
    }

    const wrappedFloor =
      leafCols.length >= 6 && isAtomic && maxAtomicPx <= 64
        ? COMPACT_NUMERIC_WRAPPED_HEADER_FLOOR_PX
        : WRAPPED_HEADER_FLOOR_PX;
    const headerFloorPx =
      headerPx <= SHORT_HEADER_SINGLE_LINE_PX
        ? headerPx
        : Math.max(wrappedFloor, Math.ceil(headerPx * 0.56));

    cellWidths.sort((a, b) => a - b);
    const p90CellPx = percentile(cellWidths, 0.9);
    const p95CellPx = percentile(cellWidths, 0.95);

    const textMinPx = Math.min(
      140,
      Math.max(Math.min(headerFloorPx, 110), maxUnbreakablePx),
    );
    const minPx = isAtomic
      ? Math.max(56, Math.min(240, Math.max(headerFloorPx, maxAtomicPx)))
      : Math.max(colIdx === 0 ? FIRST_TEXT_COLUMN_FLOOR_PX : 56, textMinPx);

    const maxCapRatio = leafCols.length <= 5 && colIdx === 0 ? 0.34 : 0.38;
    const maxCapPx = Math.max(minPx, availablePx * maxCapRatio);
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
      maxCapPx,
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
  if (explicitPctSum > 90) {
    const scale = 90 / explicitPctSum;
    for (const b of bounds) {
      if (b.explicitPct !== undefined) {
        b.explicitPct *= scale;
      }
    }
    explicitPctSum = 90;
  }

  // Explicit widths are hints, not permission to create an unreadable column.
  // Apply the same pixel floor used by flex compression before normalization.
  explicitPctSum = 0;
  for (const b of bounds) {
    if (b.explicitPct !== undefined) {
      b.explicitPct = Math.max(b.explicitPct, explicitFloorPct(b, availablePx));
      explicitPctSum += b.explicitPct;
    }
  }

  if (flexBounds.length === 0) {
    const pixels = bounds.map(
      (b) => availablePx * ((b.explicitPct ?? 0) / 100),
    );
    const totalPx = pixels.reduce((sum, width) => sum + width, 0) || 1;
    return {
      columnCssWidths: formatPercentWidths(
        pixels.map((width) => (width / totalPx) * 100),
      ),
      minTableWidthPx:
        totalPx > availablePx + 4 ? Math.ceil(totalPx) : undefined,
    };
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

    // Pass 2: Grow columns absorb surplus first, but a single text column must
    // not swallow the free space in a sparse table. Leftovers are then shared
    // by ideal width with every column that can still expand, including Fit.
    if (surplus > 0.5) {
      const growCols = flexBounds.filter((b) => b.sizing === "grow");
      const growWeight = growCols.reduce((sum, b) => sum + b.weight, 0);
      if (growWeight > 0) {
        let spent = 0;
        for (const b of growCols) {
          const curr = alloc.get(b.index) ?? b.minPx;
          const room = Math.max(0, b.maxCapPx - curr);
          const add = Math.min(room, surplus * (b.weight / growWeight));
          alloc.set(b.index, curr + add);
          spent += add;
        }
        surplus -= spent;
      }

      const expanding = new Set(flexBounds.map((b) => b.index));
      while (surplus > 0.5 && expanding.size > 0) {
        let idealSum = 0;
        for (const b of flexBounds) {
          if (!expanding.has(b.index)) continue;
          const curr = alloc.get(b.index) ?? b.minPx;
          if (b.sizing === "grow" && b.maxCapPx - curr <= 0.1) {
            expanding.delete(b.index);
            continue;
          }
          idealSum += b.idealPx;
        }
        if (idealSum <= 0 || expanding.size === 0) break;

        let spent = 0;
        for (const b of flexBounds) {
          if (!expanding.has(b.index)) continue;
          const curr = alloc.get(b.index) ?? b.minPx;
          const room =
            b.sizing === "grow"
              ? Math.max(0, b.maxCapPx - curr)
              : Number.POSITIVE_INFINITY;
          const add = Math.min(room, surplus * (b.idealPx / idealSum));
          alloc.set(b.index, curr + add);
          spent += add;
          if (b.sizing === "grow" && curr + add >= b.maxCapPx - 0.1) {
            expanding.delete(b.index);
          }
        }
        if (spent <= 0.01) break;
        surplus -= spent;
      }

      // A table made exclusively of capped Grow columns still has to fill its
      // container. Only in that degenerate case, relax the caps for the residue.
      if (surplus > 0.5 && flexBounds.every((b) => b.sizing === "grow")) {
        const idealSum = flexBounds.reduce((sum, b) => sum + b.idealPx, 0);
        if (idealSum > 0) {
          for (const b of flexBounds) {
            const curr = alloc.get(b.index) ?? b.minPx;
            alloc.set(b.index, curr + surplus * (b.idealPx / idealSum));
          }
        }
      }
    }
  } else if (surplus < 0) {
    // Squeeze non-atomic text columns first before touching atomic numeric/badge columns.
    // Higher column weights resist compression via inverse-sqrt weighting.
    const textFloorPx = (b: ColumnBounds) =>
      Math.min(b.minPx, compressionFloorPx(b));
    let deficit = -surplus;
    const textCols = flexBounds.filter((b) => !b.atomic);
    const shrinkables = new Set(textCols.map((b) => b.index));
    while (deficit > 0.1 && shrinkables.size > 0) {
      let weightedRoomSum = 0;
      for (const b of textCols) {
        if (!shrinkables.has(b.index)) continue;
        const curr = alloc.get(b.index) ?? b.minPx;
        const room = Math.max(0, curr - textFloorPx(b));
        if (room <= 0.1) {
          shrinkables.delete(b.index);
          continue;
        }
        weightedRoomSum += room / Math.sqrt(Math.max(0.25, b.weight));
      }
      if (weightedRoomSum <= 0 || shrinkables.size === 0) break;

      let cutSum = 0;
      for (const b of textCols) {
        if (!shrinkables.has(b.index)) continue;
        const curr = alloc.get(b.index) ?? b.minPx;
        const room = Math.max(0, curr - textFloorPx(b));
        const invWeight = 1 / Math.sqrt(Math.max(0.25, b.weight));
        const share = deficit * ((room * invWeight) / weightedRoomSum);
        const cut = Math.min(room, share);
        alloc.set(b.index, curr - cut);
        cutSum += cut;
        if (curr - cut <= textFloorPx(b) + 0.1) {
          shrinkables.delete(b.index);
        }
      }
      if (cutSum <= 0.01) break;
      deficit -= cutSum;
    }
    // Reclaim the optional rendering slack before allowing a dense table to
    // overflow. The real 24px padding and 56px atomic floor remain intact.
    if (deficit > 0) {
      const atomicCols = flexBounds.filter((b) => b.atomic);
      const atomicFloorPx = (b: ColumnBounds) =>
        Math.max(56, b.minPx - ATOMIC_SAFETY_RECLAIM_PX);
      const totalSafetySlack = atomicCols.reduce((sum, b) => {
        const curr = alloc.get(b.index) ?? b.minPx;
        return sum + Math.max(0, curr - atomicFloorPx(b));
      }, 0);
      if (totalSafetySlack > 0) {
        const shrinkRatio = Math.min(1, deficit / totalSafetySlack);
        for (const b of atomicCols) {
          const curr = alloc.get(b.index) ?? b.minPx;
          const room = Math.max(0, curr - atomicFloorPx(b));
          const cut = room * shrinkRatio;
          alloc.set(b.index, curr - cut);
          deficit -= cut;
        }
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
  const flexPctBudget = Math.max(0, 100 - explicitPctSum);

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
