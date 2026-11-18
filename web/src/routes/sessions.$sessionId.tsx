import { createFileRoute } from "@tanstack/react-router";

export const Route = createFileRoute("/sessions/$sessionId")({
  component: SessionPage,
});

function SessionPage() {
  const { sessionId } = Route.useParams();

  return (
    <section className="mx-auto w-full max-w-6xl px-6 py-10">
      <p className="text-xs font-medium uppercase tracking-wider text-muted-foreground">
        Session
      </p>
      <h1 className="mt-2 font-heading text-2xl font-semibold">{sessionId}</h1>
      <p className="mt-3 text-sm text-muted-foreground">
        Session transcript, tool trace and questionnaire placeholders will be
        wired here.
      </p>
    </section>
  );
}
