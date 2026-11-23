import { useEffect } from "react";
import { QueryClient, QueryClientProvider } from "@tanstack/react-query";
import {
  createRootRoute,
  Link,
  Outlet,
  useNavigate,
  useRouterState,
} from "@tanstack/react-router";
import { LoaderCircleIcon } from "lucide-react";
import { AppSidebar } from "@/components/layout/AppSidebar";
import { SidebarInset, SidebarProvider } from "@/components/ui/sidebar";
import { Toaster } from "@/components/ui/sonner";
import { TooltipProvider } from "@/components/ui/tooltip";
import { AuthProvider, useAuth } from "@/hooks/useAuth";
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

function isStandalonePublicRoute(pathname: string): boolean {
  return (
    pathname === "/login" ||
    pathname === "/register" ||
    pathname.endsWith("/print") ||
    pathname.endsWith("/preview")
  );
}

function isAuthRoute(pathname: string): boolean {
  return pathname === "/login" || pathname === "/register";
}

function RootShell() {
  const pathname = useRouterState({
    select: (state) => state.location.pathname,
  });
  const navigate = useNavigate();
  const { isAuthenticated, isInitializing } = useAuth();
  const prefs = useUiPreferences();

  const publicRoute = isStandalonePublicRoute(pathname);
  const authRoute = isAuthRoute(pathname);

  useEffect(() => {
    if (isInitializing) {
      return;
    }
    if (!isAuthenticated && !publicRoute) {
      void navigate({ to: "/login", replace: true });
    } else if (isAuthenticated && authRoute) {
      void navigate({ to: "/", replace: true });
    }
  }, [authRoute, isAuthenticated, isInitializing, navigate, publicRoute]);

  const handleLeftOpenChange = (open: boolean) => {
    updatePreferences({ leftSidebarOpen: open });
  };

  if (publicRoute) {
    return <Outlet />;
  }

  if (isInitializing || !isAuthenticated) {
    return (
      <div className="flex h-svh items-center justify-center bg-background">
        <LoaderCircleIcon className="size-5 animate-spin text-muted-foreground" />
      </div>
    );
  }

  return (
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
}

function RootLayout() {
  return (
    <QueryClientProvider client={queryClient}>
      <AuthProvider>
        <TooltipProvider>
          <RootShell />
          <Toaster />
        </TooltipProvider>
      </AuthProvider>
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
