# TYPED JSON BOUNDARIES FOR RUNTIME AND REPORT APIS

## Metadata

- **Status:** Accepted
- **Date:** 2026-11-23
- **Promoted To:** `../docs/TOOL_DESCRIPTIONS.md`, `../docs/RUNTIME_CHART_PREVIEW.md`, `../docs/REPORT_LINT_RULES.md`

## 1. Context & Problem

Several externally visible receipts, browser calls and error envelopes are assembled with `json!()`
and consumed through string-key lookups. Field renames therefore compile even when producers and
consumers drift. ECharts must retain its open-ended option object, but lint and table extraction only
need a stable structural subset.

The migration must preserve exact JSON field names and optional-field behavior. Test fixtures may
continue using `json!()` because they construct protocol input rather than production payloads.

## 2. Rejected Options & Why

- **Replace `json!()` with hand-built `serde_json::Map`:** rejected because it only removes the macro
  while preserving stringly typed fields and runtime-only typo detection.
- **Model every ECharts style property:** rejected because ECharts has a large open option surface and
  a closed Rust model would discard valid renderer options.
- **Serialize typed receipts back into `Value` at every tool:** rejected where the tool framework can
  serialize a concrete return type directly; unnecessary intermediate values recreate the old boundary.
- **Change wire names to Rust snake case:** rejected because existing MCP, browser and FC contracts use
  documented camelCase or mixed names and must remain compatible.

## 3. Chosen Approach & Trade-offs

Use serializable request/receipt/envelope structs at production boundaries, with explicit serde rename
and skip rules where the wire contract requires them. Reuse the existing document and diagnostic types
instead of duplicating their fields. Browser evaluation results are deserialized into concrete views.

For ECharts, keep canonical storage as `Value` and introduce a typed structural view for the fields
used by inspection and data-table projection. A one-or-many adapter handles ECharts components that
accept either an object or an array. Unknown properties remain in the stored option and are never
rewritten by the typed view.

The trade-off is more Rust type definitions and conversion code. In return, field existence and types
are checked by serde and the compiler, while the open ECharts tail remains fully compatible.
