import path from "node:path";
import tailwindcss from "@tailwindcss/vite";
import { tanstackRouter } from "@tanstack/router-plugin/vite";
import react from "@vitejs/plugin-react";
import { defineConfig } from "vite";

// https://vite.dev/config/
export default defineConfig({
  plugins: [
    // The router plugin must run before React so file routes are generated first.
    tanstackRouter({
      target: "react",
      autoCodeSplitting: true,
    }),
    react(),
    tailwindcss(),
  ],
  resolve: {
    alias: {
      "@": path.resolve(import.meta.dirname, "./src"),
    },
  },
  server: {
    watch: {
      ignored: ["**/node_modules/**", "**/.git/**", "**/target/**"],
      usePolling: false,
    },
    // Bind all interfaces so the dev server is reachable from the LAN and from
    // the Playwright MCP container via host.docker.internal.
    host: "0.0.0.0",
    allowedHosts: true,
    proxy: {
      // Unified Quire backend: documents, sessions, auth, styles and admin APIs.
      "/api": "http://127.0.0.1:8081",
      "/mcp": "http://127.0.0.1:8081",
      "/attachments": "http://127.0.0.1:8081",
    },
  },
});
