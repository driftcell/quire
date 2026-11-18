//! Self-contained protocol guidance for cold-start agents.

use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum GuideTopic {
    Quickstart,
    TablesAndCharts,
    CitationsAndCss,
    ParallelWorkflow,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
pub struct GuideReceipt {
    pub topic: GuideTopic,
    pub guide: String,
    pub recommended_tools: Vec<String>,
    pub _next_hint: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum GuideWorkflowStep {
    Init,
    Schema,
    Author,
    Verify,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
pub struct GuideWorkflow {
    pub steps: Vec<GuideWorkflowStep>,
    pub summary: String,
}

pub fn guide_workflow() -> GuideWorkflow {
    GuideWorkflow {
        steps: vec![
            GuideWorkflowStep::Init,
            GuideWorkflowStep::Schema,
            GuideWorkflowStep::Author,
            GuideWorkflowStep::Verify,
        ],
        summary: "INIT -> SCHEMA -> AUTHOR -> VERIFY. Create or discover a report, read the digest/Markdown, mutate with expected_revision, then lint and re-read.".into(),
    }
}

pub fn guide(topic: GuideTopic) -> GuideReceipt {
    match topic {
        GuideTopic::Quickstart => quickstart(),
        GuideTopic::TablesAndCharts => tables_and_charts(),
        GuideTopic::CitationsAndCss => citations_and_css(),
        GuideTopic::ParallelWorkflow => parallel_workflow(),
    }
}

fn quickstart() -> GuideReceipt {
    GuideReceipt {
        topic: GuideTopic::Quickstart,
        guide: r##"QUIRE REPORT PROTOCOL — QUICKSTART

Workflow: INIT -> SCHEMA -> AUTHOR -> VERIFY.

1. INIT
   - Send `X-Session-Id` on MCP HTTP requests so the frontend can discover your report.
   - Call `create_report(title, idempotency_key?)`. It returns `doc_id` and `revision`.
   - If you already have a doc, call `list_reports` and `get_document_digest(doc_id)`.

2. SCHEMA
   - Read `read_as_markdown(doc_id)` or `get_document_digest(doc_id)` before editing.
   - Block IDs are short and semantic (`sec_intro`, `b_01`), never UUIDv4, unique in the document.
   - Every block supports `class_name`, `style` (`variant`, `keep_together`, `page_break_before`, `column_widths`), `source_ids`, `source_note`.

3. AUTHOR
   - Mutate only through `apply_operations(doc_id, expected_revision, summary, ops)`.
   - Ops: insert_after, insert_before, append, patch_block, replace_range, delete_blocks, move_block.
   - `expected_revision` enables Smart CAS: disjoint block edits rebase automatically; overlapping edits return conflict details with latest blocks.
   - Patch block text/style/source IDs; use `option_patch` for charts, `append_rows` / `update_cell` for tables.
   - Register citations with `upsert_sources`, or pass an inline `sources` map to `apply_operations` / `apply_draft` so registration commits atomically with the blocks. Inline syntax is `[^source_id]`.

4. VERIFY
   - `lint_document(doc_id)` returns self-healing diagnostics. Fix every error before declaring completion.
   - `read_as_markdown(doc_id)` gives block anchors, flattened tables and expanded chart data for review.
   - `GET /api/docs/{doc_id}` supplies canonical JSON plus `computed.table_layouts`, `computed.citation_map`, `computed.bibliography`, `computed.toc_tree` to the external frontend.

Rules:
- Do not invent IDs. On lookup failure use the `hint`/`valid_values` in the error.
- Put visual titles in the outer block `title` ONLY. Never duplicate them inside ECharts `option.title` or `CustomHtml` headings.
- Tables/figures are auto-numbered (`表 N` / `图 N`); never hardcode `表 1` or `图 1` in a title.
- Never send HTML/Tiptap rendering data; the frontend renders canonical JSON.
- Headings drive the TOC. Use levels 1..6 without skipping.
- Tables use recursive ColumnNode trees; never compute colspan/rowspan yourself.
"##.into(),
        recommended_tools: vec![
            "create_report".into(),
            "get_document_digest".into(),
            "apply_operations".into(),
            "lint_document".into(),
        ],
        _next_hint: "Call create_report (or list_reports), then get_document_digest before your first mutation.".into(),
    }
}

fn tables_and_charts() -> GuideReceipt {
    GuideReceipt {
        topic: GuideTopic::TablesAndCharts,
        guide: r##"TABLES AND CHARTS

TABLE BLOCK EXAMPLE
{
  "id": "financials",
  "type": "table",
  "title": "Revenue by region",
  "numbering": "table",
  "note": "Audited results for the fiscal year.",
  "columns": [
    { "key": "region", "label": "Region", "data_type": "text", "sizing": "fit" },
    { "key": "revenue", "label": "Revenue", "children": [
      { "key": "q1", "label": "Q1", "data_type": "currency", "sizing": "grow", "weight": 1.2 },
      { "key": "q2", "label": "Q2", "data_type": "currency", "sizing": "grow" }
    ]}
  ],
  "rows": [
    { "region": "North America", "q1": 1200000, "q2": 1500000 },
    { "region": "EMEA", "q1": 800000, "q2": 950000 }
  ],
  "summary_rows": [
    { "region": "Total", "q1": 2000000, "q2": 2450000 }
  ],
  "style": { "column_widths": { "region": "24%" } }
}

Rules:
- Leaf columns require unique `key`. Group nodes may omit `key`.
- `data_type`: text | number | currency | percent | date | badge.
- `sizing`: fit (P90 content width) | grow (weighted share). Defaults: fit, weight 1.0.
- Alignment is inferred and can be overridden with `align` / `header_align`.
- Title rules: put the title in outer `title` ONLY (the alias `caption` is accepted for legacy input). Numbering is automatic (`表 N` / `图 N`); never hardcode it. `note` is an explanatory footnote.
- Compact shorthand is accepted: `{"headers":["Region","Revenue"],"rows":[["EMEA","$900"]]}`. Column keys/types are inferred.
- The server pre-computes spans and normalized `<colgroup>` percentages in `computed.table_layouts`; frontends must not recalculate them.
- Table patch: `{"type":"patch_block","block_id":"financials","patch":{"append_rows":[...],"update_cell":{"row_index":0,"column_key":"q1","value":1250000}}}`. The same `patch` object can set outer `title` (legacy alias `caption`), `numbering` and `note` for tables, charts and custom HTML.

ECHARTS BLOCK EXAMPLE (dual axis)
{
  "id": "revenue-chart",
  "type": "echarts",
  "title": "Revenue and margin",
  "numbering": "figure",
  "note": "Margins are unaudited.",
  "height": 360,
  "mode": "preset",
  "preset": "bar",
  "option_patch": {
    "xAxis": { "data": ["Q1", "Q2"] },
    "yAxis": [
      { "type": "value", "name": "Revenue" },
      { "type": "value", "name": "Margin", "max": 100 }
    ],
    "series": [
      { "name": "Revenue", "type": "bar", "data": [120, 150] },
      { "name": "Margin", "type": "line", "yAxisIndex": 1, "data": [62.5, 64.0] }
    ]
  },
  "fallback_summary": "Revenue rose from 120 to 150 while gross margin increased from 62.5% to 64.0%."
}

Rules:
- Put the title in outer `title` ONLY; never duplicate it inside ECharts `option.title`. A center donut KPI (`top: "center"`) is the only allowed inner title.
- `fallback_summary` is mandatory and must state the main numeric takeaway.
- Simple skeletons can use `mode: "preset"` + `preset` (bar, horizontal_bar, line, area, pie, donut, scatter) + `option_patch`.
- Complex charts should use `mode: "option"` with a complete ECharts 6.0 `option`. When `mode` is omitted, providing `option` without `preset` is automatically inferred as option mode.
- ECharts 6.0 option mode supports `dataset` (2D arrays or object rows, multiple datasets), multi-axis Cartesian series, `matrix`, `radar`, `sankey`, `graph`, `chord`, `beeswarm`, `treemap`, `sunburst`, `tree`, `candlestick`, `boxplot`, `heatmap` and extension series such as `wordCloud`, `liquidFill` and `bar3D`.
- Formatter strings are allowed and encouraged, for example `"formatter": "{b}: {c} ({d}%)"`. JavaScript callbacks are forbidden: strings containing `function(` or `=>` fail lint with `ECHARTS_JS_FUNCTION_STRING`.
- Static-report chrome is removed automatically: `toolbox` is stripped during normalization/normalization warnings. Do not design interactions that require it.
- Indexed series must stay in range: `xAxisIndex`, `yAxisIndex`, `gridIndex`, `polarIndex` and `radarIndex` are validated against their configured arrays. A series with `coordinateSystem: "matrix"` requires a top-level `matrix` object.
- Every chart needs a real data source: `series[*].data`, `series[*].links`/`edges`, or a non-empty `dataset.source`. Otherwise lint reports `ECHARTS_DATA_MISSING`.
- Patch chart options with `{"type":"patch_block","patch":{"option":{...}}}` to replace the base option, or `option_patch` to deep-merge. The same patch can update `mode`, `preset`, `height`, `title`, `numbering`, `note` and `fallback_summary`.
- `read_as_markdown(include_chart_data=true)` expands dataset, Cartesian, relation, radar and hierarchy data into tables for reviewers.
- `GET /api/docs/{doc_id}` exposes `computed.chart_requirements[block_id].required_maps` and `.required_extensions` so the frontend can register maps/extensions before rendering.
- Every table/chart block needs `id`, and charts need non-empty `fallback_summary`.
"##.into(),
        recommended_tools: vec![
            "read_blocks".into(),
            "apply_operations".into(),
            "read_as_markdown".into(),
        ],
        _next_hint: "Insert table/chart blocks with apply_operations, then run lint_document and read_as_markdown.".into(),
    }
}

fn citations_and_css() -> GuideReceipt {
    GuideReceipt {
        topic: GuideTopic::CitationsAndCss,
        guide: r##"CITATIONS AND STYLING

SOURCES
- Register with `upsert_sources(doc_id, sources)`:
  [{ "id":"sec10", "title":"Annual filing 10-K", "url":"https://...", "authors":["Jane Smith"], "date":"2025-02-01" }]
- Dedup key: URL first (normalized), otherwise normalized title. The response's `canonical_id_map` tells you which source_id to cite.
- Citations: inline `[^sec10]`, or block-level `source_ids:["sec10"]`.
- Inline citations are numbered by first appearance. Bibliography order follows that numbering.
- `configure_references(doc_id, enabled=true, style="author_year")` switches labels to (Smith, 2025).
- Invalid source ids produce `DANGLING_SOURCE_ID` with the full valid source_id list.

THEME TOKENS
{
  "preset": "editorial",
  "tokens": {
    "--rpt-primary": "#1d4ed8",
    "--rpt-accent": "#0f766e",
    "--rpt-font-body": "Georgia, serif"
  }
}
- Token names must start with `--`; values cannot contain `; { } < > \`.
- Common tokens: --rpt-primary, --rpt-accent, --rpt-ink, --rpt-muted, --rpt-line, --rpt-surface, --rpt-background, --rpt-font-body, --rpt-font-heading, --rpt-radius, --rpt-content-width.

CUSTOM CSS
- `update_style_and_css(doc_id, custom_css=".rpt-callout { border-left: 4px solid var(--rpt-accent); }", css_mode="append")`.
- The frontend scopes CSS under `.report-root`; do not add selectors for html/body.
- The server validates braces, strings and comments and reports exact line/column.
- `css_mode="replace"` replaces custom_css; tokens_patch merges into existing tokens.

STYLE FIELDS
- `class_name` adds frontend classes.
- `style.variant` is a renderer hint.
- `keep_together` prevents block splitting across pages.
- `page_break_before` forces a new page.
- `style.column_widths` accepts %, px, rem, em, ch, cm, mm, in, pt.
"##.into(),
        recommended_tools: vec![
            "upsert_sources".into(),
            "configure_references".into(),
            "update_style_and_css".into(),
            "lint_document".into(),
        ],
        _next_hint: "Register sources before attaching citations, then lint to catch dangling IDs early.".into(),
    }
}

fn parallel_workflow() -> GuideReceipt {
    GuideReceipt {
        topic: GuideTopic::ParallelWorkflow,
        guide: r##"PARALLEL MULTI-AGENT WORKFLOW

OWNERSHIP
- The Lead creates the report and chapter placeholder headings.
- Each writing sub-agent owns a disjoint set of block IDs (for example `ch1_*` vs `ch2_*`).
- Reading is always allowed; mutations must use apply_operations with the latest expected_revision.

SMART CAS
- Pass the revision you read. If another agent committed first, the server inspects intervening revisions.
- Disjoint block IDs -> automatic rebase; your batch commits at current_rev + 1.
- Overlapping IDs -> REVISION_CONFLICT with `current_revision`, `conflicting_block_ids`, `conflicting_revisions`, and `latest_blocks`.
- Recovery: re-read digest/Markdown, patch the returned latest block (or re-send expected_revision=current_revision) and retry.

OFFLINE DRAFTS
- Sub-agents may write `draft.json` (same ReportDocument JSON shape, optional `doc_id`/`title`).
- `read_as_markdown(draft_file_path=...)` validates/project without touching the database.
- `apply_draft(draft_file_path, target_doc_id, mode="append_section")` merges atomically enough for review; empty or colliding block IDs are auto-assigned and returned in `assigned_ids`. Mutation receipts are compact: `{new_revision, assigned_ids, inline_warnings, _next_hint}`.
- Source URLs/titles are deduplicated; use `canonical_id_map` for citations.

REVIEW LOOP
1. `read_as_markdown(doc_id, section_heading=...)` for the section under review.
2. `lint_document(doc_id)`; fix dangling citations, duplicate table keys, unknown row keys, invalid CSS.
3. Patch specific blocks via block IDs from the Markdown anchors.
4. `rollback_revision(doc_id, target_revision)` creates a new monotonic revision; never rewrites history.

FRONTEND CONTRACT
- Frontend is read-only. It consumes GET /api/docs/{doc_id} canonical JSON + computed metadata and SSE revision deltas.
- SSE events: revision, ephemeral_preview, agent_presence, sync_required, plus heartbeat comments.
- Do not send cursor/presence edits other than optional ephemeral previews.
"##.into(),
        recommended_tools: vec![
            "apply_draft".into(),
            "apply_operations".into(),
            "get_document_digest".into(),
            "rollback_revision".into(),
        ],
        _next_hint: "Assign disjoint block ID prefixes before launching parallel writers, then merge drafts through apply_draft.".into(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn all_guides_are_non_empty_and_have_next_hints() {
        for topic in [
            GuideTopic::Quickstart,
            GuideTopic::TablesAndCharts,
            GuideTopic::CitationsAndCss,
            GuideTopic::ParallelWorkflow,
        ] {
            let receipt = guide(topic);
            assert!(receipt.guide.len() > 200);
            assert!(!receipt._next_hint.is_empty());
            assert!(!receipt.recommended_tools.is_empty());
        }
    }
}
