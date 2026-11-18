import { createFileRoute } from "@tanstack/react-router";

export const Route = createFileRoute("/")({
  component: HomePage,
});

function HomePage() {
  return (
    <section className="mx-auto w-full max-w-3xl px-6 py-16">
      <h1 className="font-heading text-3xl font-semibold tracking-tight">
        Quire
      </h1>
      <p className="mt-3 max-w-xl text-sm text-muted-foreground">
        Report workspace for collaborative, agent-assisted research. The shell
        is in place; report rendering and session chat arrive next.
      </p>
    </section>
  );
}
