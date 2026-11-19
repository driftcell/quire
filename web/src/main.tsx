import { createRouter, RouterProvider } from "@tanstack/react-router";
import { StrictMode } from "react";
import { createRoot } from "react-dom/client";
import { routeTree } from "./routeTree.gen";
import "./index.css";

const router = createRouter({
  routeTree,
  defaultPreload: "intent",
  defaultPreloadStaleTime: 0,
  scrollRestoration: true,
});

declare module "@tanstack/react-router" {
  interface Register {
    router: typeof router;
  }
}

const rootElement = document.getElementById("root");

if (!rootElement) {
  throw new Error("Missing #root element");
}

// Mobile report readers should keep a stable page scale. iOS Safari still
// exposes pinch gestures even with `user-scalable=no`, so stop them explicitly.
if (
  typeof window !== "undefined" &&
  window.matchMedia("(pointer: coarse)").matches
) {
  const preventGestureZoom = (event: Event) => {
    event.preventDefault();
  };
  document.addEventListener("gesturestart", preventGestureZoom, {
    passive: false,
  });
  document.addEventListener("gesturechange", preventGestureZoom, {
    passive: false,
  });
}

createRoot(rootElement).render(
  <StrictMode>
    <RouterProvider router={router} />
  </StrictMode>,
);
