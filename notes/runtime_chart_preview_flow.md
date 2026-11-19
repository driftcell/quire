# RUNTIME CHART PREVIEW FLOW

## Metadata

- **Status:** Accepted
- **Date:** 2026-11-19
- **Promoted To:** `../docs/RUNTIME_CHART_PREVIEW.md`

## 1. Context & Problem

Agents that author ECharts blocks previously had to write them into a report before anyone could see
the rendered result. Re-sending a full ECharts option for every visual tweak is expensive, error-prone
and encourages divergent copies of the same chart. The runtime therefore needs a stateless preview
step that keeps the first full option, accepts only incremental patches afterwards, and commits the
approved chart without echoing the configuration again.

The second half of the decision is structural: ECharts `mode` and `preset` abstractions made the
canonical protocol ambiguous and forced the frontend and linter to maintain preset skeletons. The
protocol now stores one native ECharts 6.0 `option`; only `option_patch` remains for incremental edits.

## 2. Rejected Options & Why

- **Re-send the complete option on every refinement.** Rejected because it dominates agent output
  tokens and makes it easy for validation, preview and commit to drift apart.
- **Create a temporary report or revision for each preview.** Rejected because previews would pollute
  revision history, Smart CAS and report listings before the chart is actually approved.
- **Keep previews only in runtime process memory.** Rejected because the runtime may have multiple
  instances and the Playwright/browser path must be able to fetch the same preview through HTTP.
- **Persist previews in PostgreSQL.** Rejected because previews are disposable, expire quickly and
  have no rollback or citation semantics; a TTL cache is the correct lifecycle.
- **Keep `mode`/`preset` while adding native `option` as another mode.** Rejected because it leaves
  two sources of truth in the canonical AST, linter, Markdown projection and frontend renderer.
- **Let `commit_chart` accept `option` as an escape hatch.** Rejected because it recreates the
  duplicate-output problem the workflow exists to prevent.

## 3. Chosen Approach & Trade-offs

Previews live in Redis under `quire:chart_preview:{preview_id}` with a 1800-second TTL. The stored
record contains the native `EchartsBlock`, computed `ChartRequirements`, data tables, optional outer
title/note and the previous `preview_id` when the chart was refined. `preview_echarts` validates the
chart with the pure protocol linter, stores the record, renders the stateless web route through
Playwright MCP and returns the new `preview_id`, lint issues and a public URL. Iterative calls must
send `based_on_preview_id` plus `option_patch`; sending `option` again is rejected. `commit_chart`
loads the stored chart and calls the server's `apply_operations` through MCP without accepting an
ECharts option.

Accepted costs: previews expire after 30 minutes, visual inspection requires
`PLAYWRIGHT_MCP_URL`, and text-only models receive only lint diagnostics and the public preview URL.
The public URL never contains the read-only token; the token stays in the URL hash for the browser
and in an Authorization header for the API request.
