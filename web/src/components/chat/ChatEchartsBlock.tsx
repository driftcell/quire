import { memo } from "react";
import { useQuery } from "@tanstack/react-query";
import { BarChart3Icon, LoaderCircleIcon } from "lucide-react";
import { getChartPreview } from "@/api/runtime";
import { EchartsCanvas } from "@/components/report/EchartsCanvas";
import type {
  ChartDataTable,
  ChartRequirements,
} from "@/contracts/report-protocol";
import type { EchartsBlock } from "@/lib/document";

export interface ResolvedChatChart {
  chart: EchartsBlock;
  tables?: ChartDataTable[];
  requirements?: ChartRequirements;
  title?: string;
  note?: string;
  sourceNote?: string;
}

export interface ChatEchartsBlockProps {
  chartId: string;
  documentCharts?: ReadonlyMap<string, ResolvedChatChart>;
}

/**
 * Renders an inline ECharts chart inside a chat message from a short embedded XML
 * `<echarts id="..."/>` tag. Resolves committed report chart blocks first and
 * falls back to fetching a cached preview record (`/api/chart-previews/{id}`).
 */
export const ChatEchartsBlock = memo(function ChatEchartsBlock({
  chartId,
  documentCharts,
}: ChatEchartsBlockProps) {
  const cleanId = chartId.trim();
  const committed = documentCharts?.get(cleanId);

  const previewQuery = useQuery({
    queryKey: ["chat-chart-preview", cleanId],
    queryFn: () => getChartPreview(cleanId),
    enabled: Boolean(cleanId) && !committed,
    retry: false,
    staleTime: 60_000,
  });

  const resolved: ResolvedChatChart | null = committed
    ? committed
    : previewQuery.data
      ? {
          chart: previewQuery.data.chart,
          tables: previewQuery.data.tables,
          requirements: previewQuery.data.requirements,
          title:
            previewQuery.data.title ??
            (typeof previewQuery.data.chart.title === "string"
              ? previewQuery.data.chart.title
              : undefined),
          note:
            previewQuery.data.note ??
            (typeof previewQuery.data.chart.note === "string"
              ? previewQuery.data.chart.note
              : undefined),
          sourceNote:
            typeof previewQuery.data.chart.source_note === "string"
              ? previewQuery.data.chart.source_note
              : undefined,
        }
      : null;

  if (!cleanId) {
    return null;
  }

  if (!resolved && previewQuery.isLoading) {
    return (
      <div
        data-slot="chat-echarts-loading"
        data-chart-id={cleanId}
        className="my-2.5 flex h-56 w-full items-center justify-center gap-2 rounded-xl border border-border/70 bg-muted/20 text-xs text-muted-foreground"
      >
        <LoaderCircleIcon className="size-4 animate-spin text-primary" />
        <span>正在加载图表 {cleanId}…</span>
      </div>
    );
  }

  if (!resolved) {
    return (
      <div
        data-slot="chat-echarts-fallback"
        data-chart-id={cleanId}
        className="my-2.5 flex items-center gap-2 rounded-xl border border-dashed border-border/80 bg-muted/25 px-3 py-2.5 text-xs text-muted-foreground"
      >
        <BarChart3Icon className="size-4 shrink-0 text-primary/80" />
        <span>
          图表 <code className="font-mono">{cleanId}</code> 已提交至报告画布
        </span>
      </div>
    );
  }

  const title =
    resolved.title ??
    (typeof resolved.chart.title === "string"
      ? resolved.chart.title
      : undefined);
  const note =
    resolved.note ??
    (typeof resolved.chart.note === "string" ? resolved.chart.note : undefined);

  return (
    <figure
      data-slot="chat-echarts"
      data-chart-id={cleanId}
      className="my-3 w-full overflow-hidden rounded-xl border border-border/80 bg-white p-3.5 text-slate-900 shadow-2xs"
    >
      {title && (
        <figcaption className="mb-2 text-center text-xs font-semibold tracking-wide text-slate-900">
          {title}
        </figcaption>
      )}
      <EchartsCanvas
        blockId={`chat-chart-${cleanId}`}
        chart={resolved.chart}
        chartTables={resolved.tables ?? []}
        requirements={resolved.requirements}
      />
      {(note || resolved.sourceNote) && (
        <div className="mt-2 space-y-0.5 border-t border-slate-200/80 pt-1.5 text-[11px] leading-4 text-slate-500">
          {note && <p>{note}</p>}
          {resolved.sourceNote && <p>资料来源：{resolved.sourceNote}</p>}
        </div>
      )}
    </figure>
  );
});
