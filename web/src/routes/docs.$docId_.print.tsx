import { useEffect, useState } from "react";
import { useQuery } from "@tanstack/react-query";
import { createFileRoute } from "@tanstack/react-router";
import { PrinterIcon } from "lucide-react";
import { getDocument } from "@/api/quire";
import { ReportViewer } from "@/components/report/ReportViewer";
import { Button } from "@/components/ui/button";

export const Route = createFileRoute("/docs/$docId_/print")({
  component: DocumentPrintPage,
});

async function waitForPrintAssets() {
  if ("fonts" in document) {
    await document.fonts.ready;
  }
  await Promise.all(
    Array.from(document.images).map((image) => {
      if (image.complete) {
        return Promise.resolve();
      }
      return new Promise<void>((resolve) => {
        image.addEventListener("load", () => resolve(), { once: true });
        image.addEventListener("error", () => resolve(), { once: true });
      });
    }),
  );
  for (let attempt = 0; attempt < 40; attempt += 1) {
    const hosts = Array.from(
      document.querySelectorAll<HTMLElement>("[data-chart-canvas]"),
    );
    if (hosts.every((host) => host.querySelector("canvas, svg"))) {
      break;
    }
    await new Promise<void>((resolve) => {
      window.requestAnimationFrame(() => resolve());
    });
  }
  await new Promise<void>((resolve) => {
    window.requestAnimationFrame(() => {
      window.requestAnimationFrame(() => resolve());
    });
  });
}

function DocumentPrintPage() {
  const { docId } = Route.useParams();
  const [pageCount, setPageCount] = useState(0);
  const [readyKey, setReadyKey] = useState<string | null>(null);
  const [printing, setPrinting] = useState(false);

  const docQuery = useQuery({
    queryKey: ["doc", docId],
    queryFn: () => getDocument(docId),
  });

  const assetKey = `${docQuery.data?.document.doc_id ?? docId}:${pageCount}`;
  const assetsReady = pageCount > 0 && readyKey === assetKey;

  useEffect(() => {
    if (!docQuery.data || pageCount === 0) {
      return undefined;
    }
    const key = `${docQuery.data.document.doc_id}:${pageCount}`;
    let cancelled = false;
    void waitForPrintAssets().then(() => {
      if (!cancelled) {
        setReadyKey(key);
      }
    });
    return () => {
      cancelled = true;
    };
  }, [docQuery.data, pageCount]);

  const title = docQuery.data?.document.cover.title || docId;

  const handlePrint = () => {
    if (!assetsReady || printing) {
      return;
    }
    setPrinting(true);
    window.requestAnimationFrame(() => {
      window.print();
      setPrinting(false);
    });
  };

  return (
    <div className="min-h-svh bg-neutral-100 px-4 py-8 print:min-h-0 print:w-[794px] print:bg-white print:p-0 print:m-0">
      <div className="mx-auto mb-6 w-full max-w-[794px] print:hidden">
        <div className="flex flex-col gap-4 border-b border-neutral-300 pb-5 sm:flex-row sm:items-end sm:justify-between">
          <div className="min-w-0">
            <h1 className="truncate text-lg font-semibold tracking-tight text-neutral-900">
              {title}
            </h1>
            <p className="mt-1 text-xs text-neutral-500 tabular-nums">
              A4 · {pageCount > 0 ? `共 ${pageCount} 页` : "正在排版"}
              <span className="mx-1.5 text-neutral-300">/</span>
              1:1 打印预览
            </p>
            <p className="mt-2 text-[11px] leading-5 text-neutral-500">
              打印设置建议：A4、100% 缩放、关闭页边距。
            </p>
          </div>
          <Button
            type="button"
            size="sm"
            disabled={!assetsReady || printing}
            onClick={handlePrint}
            className="w-full shrink-0 sm:w-auto"
          >
            <PrinterIcon className="mr-1.5 size-4" />
            {assetsReady
              ? printing
                ? "正在打开…"
                : "导出 PDF / 打印"
              : "正在准备…"}
          </Button>
        </div>
      </div>

      {docQuery.isLoading && (
        <div className="mx-auto max-w-[794px] py-12 text-center text-sm text-muted-foreground print:hidden">
          正在排版 A4 打印视图…
        </div>
      )}
      {docQuery.data && (
        <ReportViewer
          document={docQuery.data.document}
          computed={docQuery.data.computed}
          viewMode="paged"
          printMode
          onPagesReady={setPageCount}
        />
      )}
    </div>
  );
}
