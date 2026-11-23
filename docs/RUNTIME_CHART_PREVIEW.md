# RUNTIME CHART PREVIEW FLOW

## Metadata

- **Status:** Settled
- **Owner Crates:** `quire-protocol`, `quire-lint`, `quire-runtime`, `web`
- **Related Notes:** `../notes/runtime_rig_046_lifecycle_consolidation.md`, `../notes/runtime_chart_preview_flow.md`, `../notes/quire_lint_registry_split.md`, `../notes/quire_typed_json_boundaries.md`

## 1. Scope & Non-Goals

This document governs the pre-commit visual preview of a single ECharts chart and the hand-off from
the runtime agent to `quire-server`. It covers protocol validation helpers, the Redis preview record,
the runtime read-only endpoint, the stateless web route and the `preview_echarts` / `commit_chart`
tools.

It does not cover whole-report preview (`preview_report`), the report revision protocol, or ECharts
data extraction for Markdown review. Charts are always native ECharts 6.0 `option` objects; there is
no chart preset or mode abstraction.

## 2. Canonical Design & Contract

### Protocol helpers

`quire-protocol` remains synchronous and I/O-free. The preview tools reuse helpers from
`quire-lint` and `quire-protocol`:

- `quire_lint::lint_echarts_block(block_id, chart)` for isolated chart diagnostics such as missing
  data, empty fallback summaries, invalid series types/indices and JavaScript callback strings.
- `effective_chart_option(chart)` for the native `option` deep-merged with `option_patch`.
- `merge_chart_option_patch(base, patch)` for incremental deep merges (arrays replace, objects merge).
- `chart_requirements(chart)` and `chart_data_tables_for_block(chart)` for the same map/extension and
  tabular projections used by `computed.chart_requirements` and `computed.chart_tables`.
- `EchartsOptionView` for typed inspection of titles, series, axes, datasets and coordinate links.
  Canonical options and open style/data tails remain `Value`; component views preserve single-object
  versus array shapes and tolerate malformed entries so lint diagnostics are not skipped.
- Concrete runtime receipt types in `rig_agent/tool_schemas.rs`; model-facing tool and parameter
  descriptions live directly in Rust `///` doc comments on the `#[rig_tool]` definitions (see
  `TOOL_DESCRIPTIONS.md`).

### Redis preview record

`quire-runtime` stores one JSON record per preview:

- Key: `quire:chart_preview:{preview_id}`
- TTL: 1800 seconds
- Fields: `preview_id`, `chart` (`EchartsBlock`), `requirements`, `tables`, optional `title`, `note`
  and `based_on_preview_id`.

`chart.option` keeps the original complete ECharts option. Refinements accumulate in
`chart.option_patch`, so the full option is never re-sent after the first call.

### Runtime endpoint

`GET /api/chart-previews/{preview_id}` returns the record and is mounted outside the RUNTIME_API_TOKEN
router. It accepts the same `Authorization: Bearer` values as the report read API
(`QUIRE_RO_TOKEN` or `QUIRE_RW_TOKEN`), plus `RUNTIME_API_TOKEN` for local development. Missing,
malformed or expired records return `404` with `CHART_PREVIEW_NOT_FOUND`; missing credentials return
`401`.

The Vite dev proxy maps `/api/chart-previews` to `127.0.0.1:8081`.

### Web route

`/charts/{preview_id}/preview` is a stateless route. It reads `#ro_token=...` from the hash, requests
the runtime endpoint with that token, and renders `<EchartsCanvas printSafe showTools={false}>` inside
a `w-[680px]` A4-content-width container together with optional outer title and note.

The root sets:

- `data-preview-root="true"` and `data-preview-ready="true"` after fonts, the chart canvas/SVG and
  two animation frames settle.
- `data-preview-error="true"` when loading or ECharts rendering fails.
- `data-chart-canvas="true"` on the ECharts container so Playwright can verify that a canvas or SVG
  was actually produced.

ECharts animations are disabled in report charts (including previews) so screenshots never capture a
half-painted frame. Default legends are anchored to the top and the default Cartesian grid reserves top
space when a legend is visible, preventing legend/X-axis label overlap. After each render the frontend
measures the ZRender display list and nudges a single Cartesian grid horizontally so long Y-axis
labels, axis names and legends stay visually centered, while preserving the plot-area width.

#### Built-in maps

`EchartsCanvas` collects map names from `requirements.required_maps`, `geo.map` and `series[*].map`.
`web/src/lib/echartsMaps.ts` lazily imports `web/src/assets/maps/china.json` or `world.json`, checks
`echarts.getMap(name)`, and registers only missing maps. Concurrent callers share one load promise per
map and ECharts registry, so `registerMap` runs once. The map JSON is emitted as separate Vite async
chunks; the report canvas chunk contains only loader code and never the GeoJSON.

Supported names are case-insensitive and normalize to `china` / `world`. Unknown names such as
`guangdong` remain in the amber extra-registration hint. The China asset is the GeoJSON.cn 1.6.3
province collection with the full South China Sea inset/line and Hainan geometry extending below 17°N;
the world asset is the ECharts 4.9.0 world GeoJSON. Source and attribution details live in
`web/src/assets/maps/README.md`.

### Agent tools

`preview_echarts` has two modes:

1. **Create:** requires the complete native `option`; optional top-level `title`, `note`, `height`,
   `fallback_summary` and `option_patch`.
2. **Refine:** requires `based_on_preview_id` plus `option_patch` and/or top-level overrides. Sending
   `option` together with `based_on_preview_id` is rejected.

The tool validates, stores a new preview id, optionally captures a Playwright MCP screenshot, and
returns `{ preview_id, preview_url, lint_issues, lint_summary, image_included, preview_error }`.
Vision-capable models receive the screenshot as a user image with a chart-specific inspection caption.

`commit_chart` accepts only `preview_id`, `doc_id`, `block_id`, an insertion anchor (`after_block_id`,
`before_block_id`, `section_id` or append) and optional `expected_revision` / `summary`. Its schema
has no `option` or `option_patch` fields. It loads the stored chart and calls `get_document_digest` when needed plus `exec_apply_operations` directly in-process on `RuntimeToolScope::report_mcp()` (`QuireMcpServer`) with the current turn's `RequestContext`. It returns the normal mutation receipt plus
`preview_id` and `block_id`. An expired preview returns an actionable error that tells the agent to
call `preview_echarts` again.

## 3. Invariants & Verification

- `quire-protocol` performs no I/O and owns chart merge/validation helpers; `quire-lint` owns the
  static chart diagnostics.
- `quire-runtime` embeds `quire-store` and `quire-server` in-process while `quire-server` never depends on the runtime.
- A `preview_id` is valid for exactly 1800 seconds and returns `404` afterwards.
- Public preview URLs never contain the read-only token.
- The first `preview_echarts` call contains the full option; later calls contain only a patch.
- `commit_chart` cannot receive an ECharts option by construction and executes directly in-process against `QuireMcpServer` with per-call Quire identity metadata.
- Built-in map GeoJSON loads only through dynamic `import()`; the main and `EchartsCanvas` chunks
  contain none of the China/world feature data.
- `china.json` retains all 34 province-level features plus the South China Sea line, with Hainan
  geometry extending below 17°N.
- `cargo fmt --all -- --check`, `cargo clippy --workspace --all-targets -- -D warnings` and
  `cargo test --workspace` pass.
- `just schema` regenerates `contracts/report-protocol.schema.json`; `web` regenerates its TypeScript
  contracts and passes typecheck, lint and build.
