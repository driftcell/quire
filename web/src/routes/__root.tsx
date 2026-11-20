import { QueryClient, QueryClientProvider } from "@tanstack/react-query";
import {
  createRootRoute,
  Link,
  Outlet,
  useRouterState,
} from "@tanstack/react-router";
import { AppSidebar } from "@/components/layout/AppSidebar";
import { SidebarInset, SidebarProvider } from "@/components/ui/sidebar";
import { Toaster } from "@/components/ui/sonner";
import { TooltipProvider } from "@/components/ui/tooltip";
import { updatePreferences, useUiPreferences } from "@/lib/preferences";

const queryClient = new QueryClient({
  defaultOptions: {
    queries: {
      retry: 1,
      staleTime: 15_000,
    },
  },
});

export const Route = createRootRoute({
  component: RootLayout,
  notFoundComponent: NotFoundPage,
});

function RootLayout() {
  const pathname = useRouterState({
    select: (state) => state.location.pathname,
  });
  const isPrintRoute = pathname.endsWith("/print");

  const prefs = useUiPreferences();

  const handleLeftOpenChange = (open: boolean) => {
    updatePreferences({ leftSidebarOpen: open });
  };

  const content = isPrintRoute ? (
    <Outlet />
  ) : (
    <SidebarProvider
      open={prefs.leftSidebarOpen}
      onOpenChange={handleLeftOpenChange}
    >
      <AppSidebar />
      <SidebarInset className="flex h-svh min-w-0 flex-1 flex-col overflow-hidden bg-muted/30">
        <Outlet />
      </SidebarInset>
    </SidebarProvider>
  );

  return (
    <QueryClientProvider client={queryClient}>
      <TooltipProvider>
        {content}
        <Toaster />
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
