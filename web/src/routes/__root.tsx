import { QueryClient, QueryClientProvider } from "@tanstack/react-query";
import { createRootRoute, Link, Outlet } from "@tanstack/react-router";
import { TooltipProvider } from "@/components/ui/tooltip";

const queryClient = new QueryClient({
  defaultOptions: {
    queries: {
      retry: 1,
      staleTime: 30_000,
    },
  },
});

export const Route = createRootRoute({
  component: RootLayout,
  notFoundComponent: NotFoundPage,
});

function RootLayout() {
  return (
    <QueryClientProvider client={queryClient}>
      <TooltipProvider>
        <div className="flex min-h-svh flex-col bg-background text-foreground">
          <header className="border-b">
            <nav className="mx-auto flex h-14 w-full max-w-6xl items-center gap-6 px-6">
              <Link to="/" className="font-heading text-base font-semibold">
                Quire
              </Link>
              <Link
                to="/docs/$docId"
                params={{ docId: "example" }}
                className="text-sm text-muted-foreground hover:text-foreground"
              >
                Reports
              </Link>
              <Link
                to="/sessions/$sessionId"
                params={{ sessionId: "example" }}
                className="text-sm text-muted-foreground hover:text-foreground"
              >
                Sessions
              </Link>
            </nav>
          </header>
          <main className="flex-1">
            <Outlet />
          </main>
        </div>
      </TooltipProvider>
    </QueryClientProvider>
  );
}

function NotFoundPage() {
  return (
    <div className="mx-auto w-full max-w-3xl px-6 py-16">
      <h1 className="font-heading text-2xl font-semibold">Not found</h1>
      <p className="mt-2 text-sm text-muted-foreground">
        This route does not exist.
      </p>
      <Link to="/" className="mt-6 inline-block text-sm underline">
        Back to Quire
      </Link>
    </div>
  );
}
