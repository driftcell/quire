import { useQuery } from "@tanstack/react-query";
import { createFileRoute } from "@tanstack/react-router";
import { PrinterIcon } from "lucide-react";
import { getDocument } from "@/api/quire";
import { ReportViewer } from "@/components/report/ReportViewer";
import { Button } from "@/components/ui/button";

export const Route = createFileRoute("/docs/$docId/print")({
  component: DocumentPrintPage,
});

function DocumentPrintPage() {
  const { docId } = Route.useParams();

  const docQuery = useQuery({
    queryKey: ["doc", docId],
    queryFn: () => getDocument(docId),
  });

  return (
    <div className="min-h-svh bg-neutral-100 print:bg-white">
      <div className="fixed top-4 right-4 z-50 print:hidden">
        <Button
          type="button"
          size="sm"
          onClick={() => window.print()}
          className="shadow-md"
        >
          <PrinterIcon className="mr-1.5 size-4" />
          导出 PDF / 打印
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
        />
      )}
    </div>
  );
}
