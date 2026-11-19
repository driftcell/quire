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

  return (
    <div className="min-h-svh bg-neutral-100 print:min-h-0 print:w-[794px] print:bg-white print:p-0 print:m-0">
      <div className="fixed top-4 right-4 z-50 flex items-center gap-3 rounded-lg border bg-background/95 px-3 py-2 shadow-md backdrop-blur print:hidden">
        <div className="hidden max-w-[42vw] text-right sm:block">
          <p className="truncate text-xs font-medium">{title}</p>
          <p className="text-[11px] text-muted-foreground tabular-nums">
            {pageCount > 0 ? `共 ${pageCount} 页` : "正在排版…"}
          </p>
        </div>
        <Button
          type="button"
          size="sm"
          disabled={!assetsReady}
          onClick={() => {
            if (assetsReady) {
              window.print();
            }
          }}
          className="shadow-sm"
        >
          <PrinterIcon className="mr-1.5 size-4" />
          {assetsReady ? "导出 PDF / 打印" : "正在准备…"}
        </Button>
      </div>

      {docQuery.isLoading && (
        <div className="p-12 text-center text-sm text-muted-foreground">
          正在排版 A4 打印视图...
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
