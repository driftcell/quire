import { useEffect, useMemo, useState } from "react";
import { useQuery } from "@tanstack/react-query";
import { createFileRoute } from "@tanstack/react-router";
import { getChartPreview } from "@/api/runtime";
import { EchartsCanvas } from "@/components/report/EchartsCanvas";

export const Route = createFileRoute("/charts/$previewId/preview")({
  component: ChartPreviewPage,
});

function readHashRoToken(): string | undefined {
  if (typeof window === "undefined") return undefined;
  const hash = window.location.hash.replace(/^#/, "");
  if (!hash) return undefined;
  const params = new URLSearchParams(hash);
  const token = params.get("ro_token")?.trim();
  return token && token.length > 0 ? token : undefined;
}

function ChartPreviewPage() {
  const { previewId } = Route.useParams();
  const roToken = useMemo(() => readHashRoToken(), []);
  const [previewReady, setPreviewReady] = useState(false);
  const [renderError, setRenderError] = useState<string | null>(null);

  const previewQuery = useQuery({
    queryKey: ["chart-preview", previewId],
    queryFn: () => getChartPreview(previewId, roToken),
    retry: false,
  });

  useEffect(() => {
    if (!previewQuery.isSuccess || !previewQuery.data) {
      return undefined;
    }
    let cancelled = false;

    const markReady = async () => {
      if ("fonts" in window.document) {
        await window.document.fonts.ready;
      }
      for (let attempt = 0; attempt < 40; attempt += 1) {
        if (
          window.document.querySelector(
            "[data-chart-canvas] canvas, [data-chart-canvas] svg",
          )
        ) {
          break;
        }
        await new Promise<void>((resolve) => {
          window.setTimeout(resolve, 50);
        });
      }
      await new Promise<void>((resolve) => {
        let settled = false;
        const finish = () => {
          if (!settled) {
            settled = true;
            resolve();
          }
        };
        window.requestAnimationFrame(() => {
          window.requestAnimationFrame(finish);
        });
        // Background/occluded tabs may throttle rAF; never block readiness forever.
        window.setTimeout(finish, 300);
      });
      if (!cancelled) {
        setPreviewReady(true);
      }
    };

    void markReady();
    return () => {
      cancelled = true;
    };
  }, [previewQuery.isSuccess, previewQuery.data]);

  const errorMessage = previewQuery.error?.message ?? renderError;
  const preview = previewQuery.data;
  const title = preview?.title ?? (typeof preview?.chart.title === "string"
    ? preview.chart.title
    : undefined);
  const note = preview?.note ?? (typeof preview?.chart.note === "string"
    ? preview.chart.note
    : undefined);

  return (
    <div
      data-preview-root="true"
      data-preview-ready={previewReady && !errorMessage ? "true" : "false"}
      data-preview-error={errorMessage ? "true" : undefined}
      data-preview-id={previewId}
      className="min-h-svh bg-neutral-100 py-10"
    >
      <div className="mx-auto w-[680px] max-w-full bg-white px-10 py-8 shadow-sm">
        {previewQuery.isLoading && (
          <div className="py-12 text-center text-sm text-muted-foreground">
            正在渲染图表预览...
          </div>
        )}
        {errorMessage && (
          <div className="rounded-lg border border-destructive/40 bg-destructive/10 p-4 text-sm text-destructive">
            图表预览加载失败：{errorMessage}
          </div>
        )}
        {preview && !errorMessage && (
          <div className="space-y-4">
            {title && (
              <h1 className="text-center text-[15px] font-semibold tracking-wide text-slate-900">
                {title}
              </h1>
            )}
            <EchartsCanvas
              blockId={`preview-${preview.preview_id}`}
              chart={preview.chart}
              chartTables={preview.tables ?? []}
              requirements={preview.requirements}
              renderer="svg"
              printSafe
              showTools={false}
              onRenderError={setRenderError}
            />
            {note && (
              <p className="text-center text-xs leading-relaxed text-slate-500">
                {note}
              </p>
            )}
          </div>
        )}
      </div>
    </div>
  );
}
