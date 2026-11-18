import { createFileRoute } from "@tanstack/react-router";

export const Route = createFileRoute("/docs/$docId/print")({
  component: PrintPage,
});

function PrintPage() {
  const { docId } = Route.useParams();

  return (
    <div className="report-root mt-10 border-t pt-8">
      <p className="text-xs font-medium uppercase tracking-wider text-muted-foreground">
        Print view
      </p>
      <h2 className="mt-2 font-heading text-xl font-semibold">{docId}</h2>
      <p className="mt-3 text-sm text-muted-foreground">
        A print-optimized projection of the report will render here.
      </p>
    </div>
  );
}
