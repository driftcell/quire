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
    // Bind all interfaces so the dev server is reachable from the LAN and from
    // the Playwright MCP container via host.docker.internal.
    host: "0.0.0.0",
    allowedHosts: true,
    proxy: {
      // quire-server: read-only report and documentation APIs.
      "/api/docs": "http://127.0.0.1:8082",
      // quire-runtime: sessions, knowledge bases, models, MCP and attachments.
      "/api/sessions": "http://127.0.0.1:8081",
      "/api/kb": "http://127.0.0.1:8081",
      "/api/models": "http://127.0.0.1:8081",
      "/api/mcp": "http://127.0.0.1:8081",
      "/attachments": "http://127.0.0.1:8081",
    },
  },
});
