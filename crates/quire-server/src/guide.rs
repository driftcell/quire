//! Self-contained protocol guidance for cold-start agents.

use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum GuideTopic {
    #[serde(alias = "start", alias = "intro", alias = "overview")]
    Quickstart,
    #[serde(
        alias = "blocks",
        alias = "tables",
        alias = "charts",
        alias = "tables_and_chart"
    )]
    TablesAndCharts,
    #[serde(alias = "citations", alias = "css", alias = "sources")]
    CitationsAndCss,
    #[serde(alias = "parallel", alias = "concurrency", alias = "agents")]
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
   - Call `create_report(title, idempotency_key?)`. Pass agreed `target_prose_chars` (e.g. 30000 for 约3万字). Returns `doc_id` and `revision`.
   - For existing reports, call `list_reports` then `get_document_digest(doc_id)`.

2. SCHEMA
   - Call `read_as_markdown(doc_id, section_id=ID)` using a heading ID from digest; `block_ids=[ID]` reads specific blocks.
   - `block_id`: short semantic slug (`sec_intro`, `c1_p01`), never UUIDv4, unique in doc.
   - `source_id`: prefer 2-3 lowercase words + 2-digit year (`moe_forum_26`, `zj_nsfc_25`) for BPE efficiency; avoid long multi-hyphen slugs.
   - All blocks accept `class_name`, `style` (`variant`, `keep_together`, `page_break_before`, `column_widths`), `source_ids`, `source_note`.

3. AUTHOR
   - Mutate only via `apply_operations(doc_id, expected_revision, summary, ops)`.
   - Ops: `insert_after`, `insert_before`, `append`, `append_to_section`, `patch_block`, `replace_range`, `delete_blocks`, `move_block`.
   - Section append: `{"type":"append_to_section","section_id":"ch1","blocks":[...]}`.
   - Smart CAS (`expected_revision`): disjoint block edits rebase automatically (`rebased: true`, `rebased_over`); overlapping edits return conflict with `latest_blocks`.
   - Receipts split diagnostics into `blocking_errors` (committed, must fix) and `inline_warnings`, scoped to touched blocks.
   - Patch in-place with `patch_block` (`option_patch` for charts; `columns`/`rows`/`summary_rows`/`append_rows`/`update_cell` for tables) to preserve `block_id` and comment anchors.
   - Register citations via `upsert_sources` or inline `sources` map on `apply_operations`. Inline syntax: `[^source_id]`.

4. VERIFY
   - `lint_document(doc_id)` returns grouped diagnostics (`code`, `rule`, `hint`) and `suppressed_blocks`. Fix all `error`s and review `warning`s.
   - Suppress via `PatchBlock.lint_ignore` only when `suppressible: true` and rule genuinely does not apply. Core/safety rules are never suppressible.
   - `read_as_markdown(doc_id)` projects `@block id [type]` anchors, flattened tables, chart tables, and a deduplicated `## Sources` list.
   - Check digest `prose_char_count` vs `target_prose_chars` (`prose_char_delta`); clean lint does not replace meeting length target.

Rules:
- Never invent IDs; use `hint`/`valid_values` on lookup errors.
- Set visual main title in outer `title` ONLY (subplot labels in `option.title` array allowed; never duplicate outer title inside `option.title` or `CustomHtml`).
- Tables/figures auto-number (`表 N` / `图 N`); never hardcode `表 1`/`图 1` in titles.
- Never send HTML/Tiptap data; frontend renders canonical JSON.
- Headings drive TOC: use levels 1..6 without skipping.
- Tables use recursive `ColumnNode` trees; never compute colspan/rowspan manually.
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
  "id": "financials", "type": "table", "title": "Revenue by region", "numbering": "table",
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
  "summary_rows": [{ "region": "Total", "q1": 2000000, "q2": 2450000 }],
  "style": { "keep_together": true }
}

COMPACT TABLE PRESET (<= 8 leaf columns)
- Omit `style.column_widths` for <= 8 leaf columns; frontend Water-Filling allocator measures formatted cell text, locks short headers/enums to 1 line, keeps numbers unbreakable, and preserves 78px min text width.
- Set `column_widths` (`%` or `px`) only for >= 9 columns or preview-verified fixes.
- Keep leaf headers 2–5 Chinese chars; move shared scopes/units into `note` or parent `ColumnNode`. For raw numeric `percent` columns (`56.0`), include `（%）` in header.
- Keep status/enum cells concise (`否（双一流）`, not full sentences); prefer `keep_together: true`.
7-column preset:
{
  "id": "benchmark_2021", "type": "table", "title": "五校科研竞争力对标（2021年）", "numbering": "table",
  "note": "单位：项、万元；资助率=立项数/申报数×100%；统计口径含面上与青年项目。",
  "columns": [
    { "key": "school", "label": "高校", "data_type": "text", "sizing": "grow", "weight": 1.2 },
    { "label": "2020年", "children": [
      { "key": "projects_2020", "label": "立项", "data_type": "number" },
      { "key": "funding_2020", "label": "经费", "data_type": "currency" }
    ]},
    { "label": "2021年", "children": [
      { "key": "projects_2021", "label": "立项", "data_type": "number" },
      { "key": "funding_2021", "label": "经费", "data_type": "currency" },
      { "key": "funding_rate", "label": "资助率", "data_type": "percent" },
      { "key": "growth_rate", "label": "增速", "data_type": "percent" }
    ]}
  ],
  "rows": [{ "school": "A校（本校）", "projects_2020": 190, "funding_2020": 15120, "projects_2021": 214, "funding_2021": 18650, "funding_rate": "18.4%", "growth_rate": "12.6%" }],
  "style": { "keep_together": true }
}

TABLE RULES
- Leaf columns require unique `key`; group nodes may omit `key`.
- `data_type`: text | number | currency | percent | date | badge.
- `sizing`: fit (P90 width) | grow (weighted share). Defaults: fit, weight 1.0.
- Put title in outer `title` ONLY (never hardcode `表 N`/`图 N`); `note` is an explanatory footnote.
- Shorthand accepted: `{"headers":["Region","Revenue"],"rows":[["EMEA","$900"]],"summary_rows":[["Total","$900"]]}`.
- Server pre-computes spans & `<colgroup>` widths in `computed.table_layouts`.
- Patch tables in-place via `patch_block` (`columns`, `rows`, `summary_rows`, `append_rows`, `update_cell`, `title`, `numbering`, `note`).

ECHARTS BLOCK EXAMPLE (dual axis)
{
  "id": "revenue-chart", "type": "echarts", "title": "Revenue and margin", "numbering": "figure",
  "note": "Margins are unaudited.", "height": 360,
  "option": {
    "tooltip": { "trigger": "axis" },
    "xAxis": { "type": "category", "data": ["Q1", "Q2"] },
    "yAxis": [{ "type": "value", "name": "Revenue" }, { "type": "value", "name": "Margin", "max": 100 }],
    "series": [
      { "name": "Revenue", "type": "bar", "data": [120, 150] },
      { "name": "Margin", "type": "line", "yAxisIndex": 1, "data": [62.5, 64.0] }
    ]
  },
  "fallback_summary": "Revenue rose from 120 to 150 while gross margin increased from 62.5% to 64.0%."
}

ECHARTS RULES
- Outer `title` ONLY; never duplicate in `option.title` (center donut KPI, subplot `option.title` array, or positioned light subtitle `<=13px` allowed).
- `fallback_summary` is required with main numeric takeaway.
- Send complete native ECharts 6.0 `option` (`dataset`, multi-axis, `matrix`, `radar`, `sankey`, `graph`, `chord`, `beeswarm`, `treemap`, `sunburst`, `tree`, `candlestick`, `boxplot`, `heatmap`, `wordCloud`, `liquidFill`, `bar3D`). Prefer 2D `dataset.source` with header row or explicit `dimensions`.
- Use formatter template strings (`"{b}: {c} ({d}%)"`); JS callbacks (`function(`/ `=>`) are forbidden. `toolbox` is stripped automatically.
- Index bounds (`xAxisIndex`, `yAxisIndex`, `gridIndex`, `datasetIndex`, `polarIndex`, `radarIndex`) are validated; `coordinateSystem: "matrix"` requires top-level `matrix`.
- Every chart needs real data (`series[*].data`, `links`/`edges`, or non-empty `dataset.source`).
- Patch via `patch_block` (`option` to replace, `option_patch` to deep-merge; switching axis to `"category"` clears stale numeric `min`/`max`/`interval`).

MULTI-PANEL CHARTS
- Use one ECharts block with a `grid` array for 1×2 (height 360) or 2×2 (height 460–520) panels sharing caption/units/legend.
- Bind `gridIndex` on `xAxis`/`yAxis`, `xAxisIndex`/`yAxisIndex` on Cartesian series, `datasetIndex` on dataset series; use `option.title` array for subplot labels.
- Pie/radar series can coexist with Cartesian grids using `center`/`radius` or `radarIndex`.
- Call `preview_echarts` and inspect screenshot before `commit_chart` to catch label/legend collisions.
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
- Register via `upsert_sources(doc_id, sources)`: `[{ "id":"sec_10k_25", "title":"Annual filing 10-K", "url":"https://...", "authors":["Jane Smith"], "date":"2025-02-01" }]`.
- Prefer concise 2-word + 2-digit year `source_id`s (`moe_forum_26`, `zj_nsfc_25`) for BPE token efficiency.
- Dedup key: normalized URL first, else normalized title; cite IDs from `canonical_id_map`. Partial updates by `id` are allowed without repeating `title`.
- `list_sources(doc_id, source_ids?)` inspects registered sources; `delete_sources(doc_id, source_ids?, prune_unused?)` removes uncited orphan sources after restructuring (never guess IDs).
- `excerpt` is optional short verbatim prose passage; omit for structured DB/API records and use `note` for statistical scope/time window.
- Read original pages before citing; never attach sources just to pad counts.
- Cite inline `[^id]` or block-level `source_ids:["id"]`; bibliography orders by first appearance. `configure_references(doc_id, enabled=true, style="author_year")` switches to (Smith, 2025).

PAGE CHROME
- `configure_page_chrome(doc_id, header={"left":"{kicker}","center":"","right":"{title}"}, footer={"left":"{authors}","center":"第 {page} 页 / 共 {total} 页","right":"{date}"}, hide_header_on_first_page=true)`.
- Slots: `left`, `center`, `right` (omit slot object to keep current). Placeholders: `{page}`, `{total}`, `{title}`, `{subtitle}`, `{date}`, `{kicker}`, `{authors}`.
- `heading_page_break`: `"none"` (default), `"h1"`, or `"h1_and_h2"`. `cover.custom_html` always occupies page 1 in paged/print mode.

THEME TOKENS & CUSTOM CSS
- Tokens: `{"preset":"default","tokens":{"--rpt-primary":"#1E3A5F","--rpt-accent":"#3B6E9C","--rpt-font-body":"Georgia, serif"}}`. Names start with `--`; no `; { } < > \`.
- Common tokens: `--rpt-primary`, `--rpt-accent`, `--rpt-ink`, `--rpt-muted`, `--rpt-border`, `--rpt-surface`, `--rpt-background`, `--rpt-font-body`, `--rpt-font-heading`, `--rpt-radius`, `--rpt-content-width`.
- `update_style_and_css(doc_id, custom_css="...", css_mode="append"|"replace")`. Scoped under `.report-root`; never target `html`/`body`. Server validates braces/strings/comments.

STYLE & ATTRIBUTION
- Block style: `class_name`, `style.variant`, `keep_together`, `page_break_before`, `style.column_widths` (`%`, `px`, `rem`, `em`, `ch`, `cm`, `mm`, `in`, `pt`).
- `table`/`echarts`/`custom_html` support `note` (footer note with `[^id]`); every block supports `source_note` (`资料来源：...` with `[^id]`) and `source_ids`. Both participate in citation numbering.
"##.into(),
        recommended_tools: vec![
            "upsert_sources".into(),
            "delete_sources".into(),
            "list_sources".into(),
            "configure_references".into(),
            "configure_page_chrome".into(),
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
- Lead creates report + chapter placeholder headings, freezes verified facts separately from conflicts/hypotheses, and completes 1 pilot chapter before parallelizing remaining writers.
- Analysts own derived metrics/tables/charts; writers own prose + disjoint block IDs (return visual requests rather than adding duplicate visuals).
- Mutations require `apply_operations` with latest `expected_revision`. Lead reads every chapter via `read_as_markdown(section_id=heading_id)` before delivery; `APPROVED` applies only to the reviewed revision.

SMART CAS
- Disjoint block IDs -> auto-rebase at `current_rev + 1` (`base_revision`, `rebased: true`, `rebased_over`).
- Overlapping IDs -> `REVISION_CONFLICT` with `current_revision`, `conflicting_block_ids`, `latest_blocks`. Re-read and retry.
- `blocking_errors` in receipt means committed but lint-dirty on touched blocks; fix before declaring done.

ONLINE AUTHORING
- Mutate only via `apply_operations` (`replace_range` on placeholders or `append_to_section`). Empty/colliding block IDs auto-assign via `assigned_ids`.
- Commit sources atomically via inline `sources` map on `apply_operations`.

REVIEW LOOP
1. `read_as_markdown(doc_id, section_id=heading_id)` using exact heading ID from digest (e.g. `ch1`, `c2-1`).
2. `lint_document(doc_id)`; fix all errors, review warnings, suppress only genuinely inapplicable `suppressible: true` rules.
3. Patch exact block IDs from `@block id [type]` anchors.
4. `rollback_revision(doc_id, target_revision)` appends a new monotonic revision.

COMMENTS
- Reviewers: `add_comment(doc_id, block_id, body, quote?)`. Writers: `reply_comment(doc_id, thread_id, body, mark_addressed=true, addressed_rev=N)` after fixing.
- Comments do not bump `current_rev` or participate in CAS. Deleted blocks keep threads with `anchor_detached=true`. Only humans can `archived` threads.

FRONTEND CONTRACT
- Read-only frontend consumes `GET /api/docs/{doc_id}`, `/diff?fromRev=A&toRev=B`, `/sync?fromRev=A`, and SSE (`revision`, `ephemeral_preview`, `agent_presence`, `ephemeral_clear`, `comment_updated`, `sync_required`).
- Pass `preview_ids` to `apply_operations` to clear ephemeral previews atomically on commit.
"##.into(),
        recommended_tools: vec![
            "apply_operations".into(),
            "get_document_digest".into(),
            "list_comments".into(),
            "reply_comment".into(),
            "rollback_revision".into(),
        ],
        _next_hint: "Assign disjoint block ID prefixes before launching parallel writers, then commit each writer's batch through apply_operations.".into(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn guide_topic_accepts_common_agent_aliases() {
        let blocks: GuideTopic =
            serde_json::from_value(serde_json::json!("blocks")).expect("blocks alias");
        let charts: GuideTopic =
            serde_json::from_value(serde_json::json!("charts")).expect("charts alias");
        let agents: GuideTopic =
            serde_json::from_value(serde_json::json!("agents")).expect("agents alias");
        assert_eq!(blocks, GuideTopic::TablesAndCharts);
        assert_eq!(charts, GuideTopic::TablesAndCharts);
        assert_eq!(agents, GuideTopic::ParallelWorkflow);
    }

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
