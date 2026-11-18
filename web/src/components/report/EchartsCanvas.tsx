import { memo, useEffect, useMemo, useRef, useState } from "react";
import * as echarts from "echarts";
import { BarChart3Icon, DownloadIcon, TableIcon } from "lucide-react";
import type {
  ChartDataTable,
  ChartRequirements,
  EchartsBlock,
  EchartsPreset,
} from "@/contracts/report-protocol";

export interface EchartsCanvasProps {
  blockId: string;
  chart: EchartsBlock;
  chartTables?: ChartDataTable[];
  requirements?: ChartRequirements;
  renderer?: "canvas" | "svg";
  printSafe?: boolean;
}

type JsonObject = Record<string, unknown>;

const ECHARTS_PRESETS: readonly EchartsPreset[] = [
  "bar",
  "line",
  "area",
  "pie",
  "donut",
  "scatter",
  "horizontal_bar",
];

function isPlainObject(value: unknown): value is JsonObject {
  return typeof value === "object" && value !== null && !Array.isArray(value);
}

function isEchartsPreset(value: unknown): value is EchartsPreset {
  return (
    typeof value === "string" &&
    ECHARTS_PRESETS.some((preset) => preset === value)
  );
}

function chartHeight(chart: EchartsBlock): number {
  return typeof chart.height === "number" ? chart.height : 360;
}

function chartFallbackSummary(chart: EchartsBlock): string {
  return typeof chart.fallback_summary === "string"
    ? chart.fallback_summary
    : "";
}

function deepMerge(base: JsonObject, patch: JsonObject): JsonObject {
  const result: JsonObject = { ...base };
  for (const [key, value] of Object.entries(patch)) {
    const existing = result[key];
    if (isPlainObject(existing) && isPlainObject(value)) {
      result[key] = deepMerge(existing, value);
    } else {
      result[key] = value;
    }
  }
  return result;
}

function presetSkeleton(preset: EchartsPreset | undefined): JsonObject {
  switch (preset) {
    case "horizontal_bar":
      return {
        tooltip: { trigger: "axis" },
        grid: { containLabel: true, left: 12, right: 16, top: 24, bottom: 16 },
        xAxis: { type: "value" },
        yAxis: { type: "category" },
      };
    case "line":
    case "area":
      return {
        tooltip: { trigger: "axis" },
        grid: { containLabel: true, left: 12, right: 16, top: 24, bottom: 16 },
        xAxis: { type: "category" },
        yAxis: { type: "value" },
      };
    case "pie":
    case "donut":
      return {
        tooltip: { trigger: "item" },
        legend: { type: "scroll", bottom: 0 },
      };
    case "scatter":
      return {
        tooltip: { trigger: "item" },
        grid: { containLabel: true, left: 12, right: 16, top: 24, bottom: 16 },
        xAxis: { type: "value" },
        yAxis: { type: "value" },
      };
    case "bar":
    default:
      return {
        tooltip: { trigger: "axis" },
        grid: { containLabel: true, left: 12, right: 16, top: 24, bottom: 16 },
        xAxis: { type: "category" },
        yAxis: { type: "value" },
      };
  }
}

function buildEffectiveOption(chart: EchartsBlock): JsonObject {
  const preset = isEchartsPreset(chart.preset) ? chart.preset : undefined;
  let option: JsonObject =
    chart.mode === "option" && isPlainObject(chart.option)
      ? { ...chart.option }
      : presetSkeleton(preset);

  if (chart.mode === "preset" && isPlainObject(chart.option)) {
    option = deepMerge(option, chart.option);
  }
  if (isPlainObject(chart.option_patch)) {
    option = deepMerge(option, chart.option_patch);
  }

  if (
    (option.xAxis !== undefined || option.yAxis !== undefined) &&
    option.grid === undefined
  ) {
    option.grid = {
      containLabel: true,
      left: 12,
      right: 16,
      top: 24,
      bottom: 16,
    };
  }
  return option;
}

function downloadCsv(table: ChartDataTable, filename: string) {
  const escape = (cell: string) =>
    cell.includes(",") || cell.includes('"') || cell.includes("\n")
      ? `"${cell.replaceAll('"', '""')}"`
      : cell;
  const lines = [
    table.headers.map(escape).join(","),
    ...table.rows.map((row) => row.map(escape).join(",")),
  ];
  const blob = new Blob(["\uFEFF" + lines.join("\n")], {
    type: "text/csv;charset=utf-8;",
  });
  const url = URL.createObjectURL(blob);
  const a = document.createElement("a");
  a.href = url;
  a.download = filename;
  a.click();
  URL.revokeObjectURL(url);
}

export const EchartsCanvas = memo(function EchartsCanvas({
  blockId,
  chart,
  chartTables = [],
  requirements,
  renderer = "canvas",
  printSafe = false,
}: EchartsCanvasProps) {
  const containerRef = useRef<HTMLDivElement>(null);
  const instanceRef = useRef<echarts.ECharts | null>(null);
  const [viewMode, setViewMode] = useState<"chart" | "table">("chart");
  const [runtimeError, setRuntimeError] = useState<string | null>(null);

  const chartHeightPx = chartHeight(chart);
  const effectiveHeight = printSafe
    ? Math.min(680, Math.max(140, chartHeightPx))
    : Math.max(140, chartHeightPx);

  const effectiveOption = useMemo(() => buildEffectiveOption(chart), [chart]);

  useEffect(() => {
    if (viewMode !== "chart") {
      return undefined;
    }
    const el = containerRef.current;
    if (!el) {
      return undefined;
    }

    let rafId = 0;
    let disposed = false;

    const mountOrUpdate = () => {
      if (disposed) return;
      const rect = el.getBoundingClientRect();
      if (rect.width <= 0 || rect.height <= 0) {
        return;
      }

      if (!instanceRef.current) {
        instanceRef.current = echarts.init(el, undefined, {
          renderer,
          devicePixelRatio: window.devicePixelRatio || 2,
        });
      }

      try {
        setRuntimeError(null);
        instanceRef.current.setOption(
          effectiveOption as echarts.EChartsOption,
          { notMerge: true, lazyUpdate: false },
        );
      } catch (err) {
        setRuntimeError(
          err instanceof Error ? err.message : "ECharts 渲染失败",
        );
      }
    };

    mountOrUpdate();

    const observer = new ResizeObserver((entries) => {
      const entry = entries[0];
      if (!entry) return;
      const { width, height } = entry.contentRect;
      if (width <= 0 || height <= 0) {
        return;
      }
      window.cancelAnimationFrame(rafId);
      rafId = window.requestAnimationFrame(() => {
        if (disposed) return;
        if (!instanceRef.current) {
          mountOrUpdate();
        } else {
          instanceRef.current.resize({
            animation: { duration: 0 },
          });
        }
      });
    });
    observer.observe(el);

    return () => {
      disposed = true;
      window.cancelAnimationFrame(rafId);
      observer.disconnect();
    };
  }, [effectiveOption, renderer, viewMode]);

  useEffect(() => {
    return () => {
      if (renderer === "canvas" || renderer === "svg") {
        instanceRef.current?.dispose();
      }
      instanceRef.current = null;
    };
  }, [renderer]);

  const missingMaps = requirements?.required_maps ?? [];
  const missingExts = requirements?.required_extensions ?? [];

  return (
    <div className="group/chart relative w-full">
      {!printSafe && chartTables.length > 0 && (
        <div className="mb-1.5 flex items-center justify-end gap-1 opacity-80 transition-opacity group-hover/chart:opacity-100 print:hidden">
          <button
            type="button"
            onClick={() => setViewMode("chart")}
            className={`inline-flex items-center gap-1 rounded px-2 py-0.5 text-[11px] ${
              viewMode === "chart"
                ? "bg-black/10 font-medium dark:bg-white/15"
                : "text-muted-foreground hover:bg-black/5"
            }`}
          >
            <BarChart3Icon className="size-3" />
            <span>图表</span>
          </button>
          <button
            type="button"
            onClick={() => setViewMode("table")}
            className={`inline-flex items-center gap-1 rounded px-2 py-0.5 text-[11px] ${
              viewMode === "table"
                ? "bg-black/10 font-medium dark:bg-white/15"
                : "text-muted-foreground hover:bg-black/5"
            }`}
          >
            <TableIcon className="size-3" />
            <span>数据表</span>
          </button>
          {chartTables[0] && (
            <button
              type="button"
              onClick={() => {
                const first = chartTables[0];
                if (first) {
                  downloadCsv(first, `${blockId}.csv`);
                }
              }}
              className="inline-flex items-center gap-1 rounded px-2 py-0.5 text-[11px] text-muted-foreground hover:bg-black/5"
            >
              <DownloadIcon className="size-3" />
              <span>CSV</span>
            </button>
          )}
        </div>
      )}

      {(missingMaps.length > 0 || missingExts.length > 0) && (
        <div className="mb-2 rounded border border-amber-500/30 bg-amber-500/10 px-2.5 py-1 text-[11px] text-amber-700 dark:text-amber-300">
          此图表依赖额外注册：
          {[...missingMaps.map((m) => `map:${m}`), ...missingExts].join(", ")}
        </div>
      )}

      {viewMode === "chart" ? (
        runtimeError ? (
          <div className="rounded border border-destructive/40 bg-destructive/5 p-3 text-xs text-destructive">
            图表渲染异常：{runtimeError}
            <div className="mt-1 text-muted-foreground">
              {chartFallbackSummary(chart)}
            </div>
          </div>
        ) : (
          <div
            ref={containerRef}
            style={{ height: `${effectiveHeight}px` }}
            className="w-full"
          />
        )
      ) : (
        <div className="space-y-3 overflow-x-auto rounded border p-3">
          {chartTables.map((tbl, idx) => (
            <div key={idx} className="space-y-1">
              <div className="text-xs font-medium text-muted-foreground">
                {tbl.title}
              </div>
              <table className="w-full border-collapse text-xs">
                <thead>
                  <tr className="border-b bg-black/[0.03] dark:bg-white/[0.05]">
                    {tbl.headers.map((h, hIdx) => (
                      <th
                        key={hIdx}
                        className="border border-black/10 px-2 py-1 text-left font-semibold dark:border-white/10"
                      >
                        {h}
                      </th>
                    ))}
                  </tr>
                </thead>
                <tbody>
                  {tbl.rows.map((row, rIdx) => (
                    <tr key={rIdx} className="border-b">
                      {row.map((cell, cIdx) => (
                        <td
                          key={cIdx}
                          className="border border-black/10 px-2 py-1 tabular-nums dark:border-white/10"
                        >
                          {cell}
                        </td>
                      ))}
                    </tr>
                  ))}
                </tbody>
              </table>
            </div>
          ))}
        </div>
      )}
    </div>
  );
});
