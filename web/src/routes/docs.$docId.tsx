import { createFileRoute, Link, Outlet } from "@tanstack/react-router";

export const Route = createFileRoute("/docs/$docId")({
  component: DocumentPage,
});

function DocumentPage() {
  const { docId } = Route.useParams();

  return (
    <section className="mx-auto w-full max-w-4xl px-6 py-10">
      <div className="flex items-start justify-between gap-4">
        <div>
          <p className="text-xs font-medium uppercase tracking-wider text-muted-foreground">
            Report
          </p>
          <h1 className="mt-2 font-heading text-2xl font-semibold">{docId}</h1>
        </div>
        <Link
          to="/docs/$docId/print"
          params={{ docId }}
          className="text-sm text-muted-foreground underline-offset-4 hover:text-foreground hover:underline"
        >
          Print view
        </Link>
      </div>
      <p className="mt-3 text-sm text-muted-foreground">
        The report canvas mounts here. Report-body styles stay scoped under
        <code className="mx-1 rounded bg-muted px-1.5 py-0.5 text-xs">
          .report-root
        </code>
        so they cannot leak into the App Shell.
      </p>
      <Outlet />
    </section>
  );
}
