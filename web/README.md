# Quire Web

Client-only SPA for Quire, built with Bun, Vite, React 19, TanStack Router and
Tailwind CSS v4.

## Stack

- **Routing** — `@tanstack/react-router` in SPA mode via
  `@tanstack/router-plugin/vite` (no TanStack Start / SSR).
- **Data** — `@tanstack/react-query`, `@microsoft/fetch-event-source`.
- **UI** — shadcn/ui (Base UI primitives), Radix-free generated components,
  Lucide icons, `streamdown` for streamed Markdown.
- **Charts** — Apache ECharts 6.
- **Contracts** — JSON Schema → TypeScript via `json-schema-to-typescript`.
- **Quality** — `oxlint --type-aware`, `oxlint-tsgolint`, Bun's built-in
  formatter and type checker.

## Commands

```bash
bun install
bun run dev          # Vite dev server with API proxies
bun run build        # production SPA bundle in dist/
bun run lint         # oxlint with type-aware rules
bun run typecheck    # bun check (TypeScript 7)
bun run format       # bun fmt
bun run format:check # bun fmt --check
bun run gen:types    # regenerate src/contracts/*.ts from ../contracts
```

Run all five verification gates from this directory:

```bash
bun run gen:types
bun run lint
bun run typecheck
bun run format:check
bun run build
```

## Development proxies

`vite.config.ts` proxies API traffic so the SPA can talk to both backends without
CORS configuration:

| Prefix         | Target                  | Owner                         |
| -------------- | ----------------------- | ----------------------------- |
| `/api`         | `http://127.0.0.1:8081` | Unified Quire backend         |
| `/mcp`         | `http://127.0.0.1:8081` | `quire-runtime`               |
| `/attachments` | `http://127.0.0.1:8081` | `quire-runtime`               |

## Layout

```text
src/
├── components/ui/   shadcn/ui primitives
├── contracts/       generated report/session protocol types
├── hooks/           shared hooks
├── lib/             utilities
├── routes/          file-based routes (routeTree.gen.ts is generated)
├── index.css        Tailwind v4 entry + shadcn theme tokens
└── main.tsx         router bootstrap
```

Report-body styles must stay under the `.report-root` scope declared in
`src/index.css`; App Shell styles must never target `.report-root` descendants
implicitly.

## TypeScript path aliases

TypeScript 7 (used by `bun check` and `oxlint-tsgolint`) removed `baseUrl`.
The `@/*` alias therefore lives in `compilerOptions.paths` and resolves from
the tsconfig directory; Vite maps the same alias in `vite.config.ts`.
