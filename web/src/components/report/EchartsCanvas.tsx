import { memo, useEffect, useMemo, useRef, useState } from "react";
import * as echarts from "echarts";
import { DownloadIcon, TableIcon } from "lucide-react";
import { toast } from "sonner";
import {
  Dialog,
  DialogContent,
  DialogHeader,
  DialogTitle,
} from "@/components/ui/dialog";
import type {
  ChartDataTable,
  ChartRequirements,
} from "@/contracts/report-protocol";
import { centerReportChartGrid } from "@/components/report/echartsGridCenter";
import {
  ensureEchartsMaps,
  extractRequiredMaps,
  isSupportedBuiltinMap,
} from "@/lib/echartsMaps";
import { errorMessage } from "@/lib/errors";
import type { EchartsBlock } from "@/lib/document";

export interface EchartsCanvasProps {
  blockId: string;
  chart: EchartsBlock;
  chartTables?: ChartDataTable[];
  requirements?: ChartRequirements;
  renderer?: "canvas" | "svg";
  printSafe?: boolean;
  showTools?: boolean;
  onRenderError?: (message: string | null) => void;
}

type JsonObject = Record<string, unknown>;

function isPlainObject(value: unknown): value is JsonObject {
  return typeof value === "object" && value !== null && !Array.isArray(value);
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

function chartSeries(option: JsonObject): JsonObject[] {
  const series = option.series;
  if (Array.isArray(series)) {
    return series.filter(isPlainObject);
  }
  return isPlainObject(series) ? [series] : [];
}

function chartHasLegend(option: JsonObject): boolean {
  if (option.legend === false) {
    return false;
  }
  if (isPlainObject(option.legend)) {
    return option.legend.show !== false;
  }
  return chartSeries(option).some(
    (series) =>
      typeof series.name === "string" && series.name.trim().length > 0,
  );
}

function applyLegendDefaults(option: JsonObject) {
  if (!chartHasLegend(option)) {
    return;
  }
  const legend = isPlainObject(option.legend) ? { ...option.legend } : {};
  if (legend.show === false || Array.isArray(option.legend)) {
    return;
  }
  const positioned = ["top", "bottom", "left", "right", "orient"].some(
    (key) => key in legend,
  );
  if (!positioned) {
    option.legend = { ...legend, top: 8, left: "center" };
  }
}

export function buildEffectiveOption(chart: EchartsBlock): JsonObject {
  let option: JsonObject = isPlainObject(chart.option)
    ? { ...chart.option }
    : {};

  if (isPlainObject(chart.option_patch)) {
    option = deepMerge(option, chart.option_patch);
  }

  applyLegendDefaults(option);
  const legendVisible = chartHasLegend(option);
  if (
    (option.xAxis !== undefined || option.yAxis !== undefined) &&
    option.grid === undefined
  ) {
    option.grid = {
      containLabel: true,
      left: 12,
      right: 16,
      top: legendVisible ? 48 : 24,
      bottom: 16,
    };
  }
  option = deepMerge(ACADEMIC_DEFAULTS, option);
  applyAxisDefaults(option, "xAxis");
  applyAxisDefaults(option, "yAxis");
  // Reports are screenshot/print artifacts: paint the final frame immediately so
  // grid centering and preview readiness never observe an animated first frame.
  option.animation = false;
  return option;
}

const ACADEMIC_DEFAULTS: JsonObject = {
  color: ["#1E3A5F", "#3B6E9C", "#6495B8", "#94B4CE", "#9A6B2F", "#475569"],
  textStyle: {
    fontFamily:
      '"Geist Variable", "PingFang SC", "Noto Sans SC", system-ui, sans-serif',
    color: "#334155",
    fontSize: 12,
  },
  tooltip: {
    backgroundColor: "rgba(255,255,255,0.96)",
    borderColor: "#CBD5E1",
    borderWidth: 1,
    textStyle: { color: "#1E293B", fontSize: 12 },
  },
  legend: {
    textStyle: { color: "#475569", fontSize: 11 },
    itemWidth: 12,
    itemHeight: 8,
  },
};

export const AXIS_LINE_DEFAULTS: JsonObject = { color: "#CBD5E1" };
export const AXIS_LABEL_DEFAULTS: JsonObject = {
  color: "#475569",
  fontSize: 11,
};
export const SPLIT_LINE_DEFAULTS: JsonObject = {
  color: "#E2E8F0",
  type: "dashed",
};

function withAxisDefaults(axis: JsonObject): JsonObject {
  const axisLine = isPlainObject(axis.axisLine) ? axis.axisLine : {};
  const axisLabel = isPlainObject(axis.axisLabel) ? axis.axisLabel : {};
  const splitLine = isPlainObject(axis.splitLine) ? axis.splitLine : {};
  const lineStyle = isPlainObject(axisLine.lineStyle) ? axisLine.lineStyle : {};
  const splitLineStyle = isPlainObject(splitLine.lineStyle)
    ? splitLine.lineStyle
    : {};
  return {
    ...axis,
    axisLine: {
      ...axisLine,
      lineStyle: { ...AXIS_LINE_DEFAULTS, ...lineStyle },
    },
    axisLabel: { ...AXIS_LABEL_DEFAULTS, ...axisLabel },
    splitLine: {
      ...splitLine,
      lineStyle: { ...SPLIT_LINE_DEFAULTS, ...splitLineStyle },
    },
  };
}

function applyAxisDefaults(option: JsonObject, key: "xAxis" | "yAxis") {
  const axis = option[key];
  if (Array.isArray(axis)) {
    option[key] = axis.map((entry) =>
      isPlainObject(entry) ? withAxisDefaults(entry) : entry,
    );
    return;
  }
  if (isPlainObject(axis)) {
    option[key] = withAxisDefaults(axis);
  }
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
  showTools = true,
  onRenderError,
}: EchartsCanvasProps) {
  const containerRef = useRef<HTMLDivElement>(null);
  const instanceRef = useRef<echarts.ECharts | null>(null);
  const [tableOpen, setTableOpen] = useState(false);
  const [runtimeError, setRuntimeError] = useState<string | null>(null);

  const chartHeightPx = chartHeight(chart);
  const effectiveHeight = printSafe
    ? Math.min(680, Math.max(140, chartHeightPx))
    : Math.max(140, chartHeightPx);
  const dialogTitle =
    typeof chart.title === "string" && chart.title.trim().length > 0
      ? chart.title.trim()
      : "图表数据";

  const effectiveOption = useMemo(() => buildEffectiveOption(chart), [chart]);
  const requiredMaps = useMemo(
    () => extractRequiredMaps(effectiveOption, requirements),
    [effectiveOption, requirements],
  );

  useEffect(() => {
    onRenderError?.(runtimeError);
  }, [onRenderError, runtimeError]);

  useEffect(() => {
    const el = containerRef.current;
    if (!el) {
      return undefined;
    }

    let resizeTimer = 0;
    let disposed = false;
    let renderGeneration = 0;

    const applyOption = (instance: echarts.ECharts) => {
      instance.setOption(effectiveOption as echarts.EChartsOption, {
        notMerge: true,
        lazyUpdate: false,
      });
      centerReportChartGrid(instance);
    };

    const mountOrUpdate = async () => {
      if (disposed) return;
      const rect = el.getBoundingClientRect();
      if (rect.width <= 0 || rect.height <= 0) {
        return;
      }

      const generation = ++renderGeneration;
      if (!instanceRef.current) {
        instanceRef.current = echarts.init(el, undefined, {
          renderer,
          devicePixelRatio: window.devicePixelRatio || 2,
        });
      }
      const instance = instanceRef.current;
      if (!instance) return;

      try {
        setRuntimeError(null);
        if (requiredMaps.length > 0) {
          await ensureEchartsMaps(echarts, requiredMaps);
          if (
            disposed ||
            generation !== renderGeneration ||
            instance.isDisposed()
          ) {
            return;
          }
        }
        applyOption(instance);
      } catch (err) {
        if (disposed || generation !== renderGeneration) {
          return;
        }
        setRuntimeError(
          err instanceof Error ? err.message : "ECharts 渲染失败",
        );
      }
    };

    void mountOrUpdate();

    const resized = () => {
      resizeTimer = 0;
      if (disposed) return;
      const instance = instanceRef.current;
      if (!instance) {
        void mountOrUpdate();
        return;
      }
      instance.resize({ animation: { duration: 0 } });
      centerReportChartGrid(instance);
    };
    const scheduleResize = () => {
      if (disposed) return;
      window.clearTimeout(resizeTimer);
      resizeTimer = window.setTimeout(resized, 100);
    };

    const observer = new ResizeObserver((entries) => {
      const entry = entries[0];
      if (!entry) return;
      const { width, height } = entry.contentRect;
      if (width <= 0 || height <= 0) {
        return;
      }
      scheduleResize();
    });
    observer.observe(el);
    window.addEventListener("resize", scheduleResize);

    return () => {
      disposed = true;
      window.clearTimeout(resizeTimer);
      observer.disconnect();
      window.removeEventListener("resize", scheduleResize);
    };
  }, [effectiveOption, renderer, requiredMaps]);

  useEffect(() => {
    return () => {
      if (renderer === "canvas" || renderer === "svg") {
        instanceRef.current?.dispose();
      }
      instanceRef.current = null;
    };
  }, [renderer]);

  const missingMaps = requiredMaps.filter(
    (name) => !isSupportedBuiltinMap(name),
  );
  const missingExts = requirements?.required_extensions ?? [];

  return (
    <div className="group/chart relative w-full">
      {showTools && chartTables.length > 0 && (
        <div className="pointer-events-none absolute top-0 right-0 z-10 flex items-center gap-1 rounded-md bg-background/85 px-1 py-0.5 opacity-0 shadow-xs backdrop-blur transition-opacity group-hover/chart:pointer-events-auto group-hover/chart:opacity-100 focus-within:pointer-events-auto focus-within:opacity-100 print:hidden">
          <button
            type="button"
            onClick={() => setTableOpen(true)}
            className="inline-flex items-center gap-1 rounded px-2 py-0.5 text-[11px] text-muted-foreground hover:bg-black/5"
          >
            <TableIcon className="size-3" />
            <span>数据表</span>
          </button>
          {chartTables[0] && (
            <button
              type="button"
              onClick={() => {
                const first = chartTables[0];
                if (!first) {
                  return;
                }
                try {
                  downloadCsv(first, `${blockId}.csv`);
                  toast.success("CSV 已开始下载");
                } catch (error) {
                  toast.error(errorMessage(error, "CSV 导出失败，请稍后重试"));
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
        <div className="mb-2 rounded border border-amber-500/30 bg-amber-500/10 px-2.5 py-1 text-[11px] text-amber-700">
          此图表依赖额外注册：
          {[...missingMaps.map((m) => `map:${m}`), ...missingExts].join(", ")}
        </div>
      )}

      {runtimeError ? (
        <div className="rounded border border-destructive/40 bg-destructive/5 p-3 text-xs text-destructive">
          图表渲染异常：{runtimeError}
          <div className="mt-1 text-muted-foreground">
            {chartFallbackSummary(chart)}
          </div>
        </div>
      ) : (
        <div
          ref={containerRef}
          data-chart-canvas="true"
          style={{ height: `${effectiveHeight}px` }}
          className="w-full select-none"
        />
      )}

      <Dialog open={tableOpen} onOpenChange={setTableOpen}>
        <DialogContent className="max-h-[80vh] max-w-[calc(100%-2rem)] gap-3 overflow-hidden sm:max-w-3xl">
          <DialogHeader className="pr-8">
            <DialogTitle>{dialogTitle}</DialogTitle>
          </DialogHeader>
          <div className="no-scrollbar max-h-[60vh] space-y-4 overflow-auto pr-1">
            {chartTables.map((tbl, idx) => (
              <div key={`${tbl.title}-${idx}`} className="space-y-1.5">
                <div className="text-xs font-medium text-slate-500">
                  {tbl.title}
                </div>
                <table className="w-full border-collapse text-xs">
                  <thead>
                    <tr className="border-b border-slate-700 bg-slate-100/80 text-left text-slate-800">
                      {tbl.headers.map((h, hIdx) => (
                        <th
                          key={hIdx}
                          className="px-2.5 py-1.5 font-semibold whitespace-nowrap"
                        >
                          {h}
                        </th>
                      ))}
                    </tr>
                  </thead>
                  <tbody>
                    {tbl.rows.map((row, rIdx) => (
                      <tr
                        key={rIdx}
                        className="border-b border-slate-200/70 even:bg-slate-50/70"
                      >
                        {row.map((cell, cIdx) => (
                          <td
                            key={cIdx}
                            className="px-2.5 py-1.5 tabular-nums whitespace-nowrap text-slate-700"
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
        </DialogContent>
      </Dialog>
    </div>
  );
});
