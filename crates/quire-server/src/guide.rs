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
   - Send `X-Session-Id` on MCP HTTP requests so the frontend can discover your report.
   - Call `create_report(title, idempotency_key?)`. If the user agreed a total length, pass `target_prose_chars` (e.g. 30000 for 约3万字). It returns `doc_id` and `revision`.
   - If you already have a doc, call `list_reports` and `get_document_digest(doc_id)`.

2. SCHEMA
   - Use the heading ID from your assignment or `get_document_digest(doc_id)` and call `read_as_markdown(doc_id, section_id=ID)` for the heading and its contents. `block_ids=[ID]` reads only the named block.
   - Block IDs are short and semantic (`sec_intro`, `b_01`), never UUIDv4, unique in the document.
   - Every block supports `class_name`, `style` (`variant`, `keep_together`, `page_break_before`, `column_widths`), `source_ids`, `source_note`.

3. AUTHOR
   - Mutate only through `apply_operations(doc_id, expected_revision, summary, ops)`.
   - Ops: insert_after, insert_before, append, append_to_section, patch_block, replace_range, delete_blocks, move_block.
   - Section append example: `{"type":"append_to_section","section_id":"ch1","blocks":[...]}`.
   - `expected_revision` enables Smart CAS: disjoint block edits rebase automatically; overlapping edits return conflict details with latest blocks.
   - Successful receipts report `base_revision`, `rebased`, and `rebased_over`. Errors are split into `blocking_errors` (committed but not lint-clean) and non-blocking `inline_warnings`, scoped to the blocks you touched.
   - Patch block text/style/source IDs; use `option_patch` for charts, and `columns` / `rows` / `summary_rows` / `append_rows` / `update_cell` on `patch_block` for in-place table updates (preserving `block_id` and review comment anchors).
   - Register citations with `upsert_sources`, or pass an inline `sources` map to `apply_operations` so registration commits atomically with the blocks. Inline syntax is `[^source_id]`.

4. VERIFY
   - `lint_document(doc_id)` returns diagnostics that carry `code`, `rule`, `hint` and `suppressible`. Fix every `error` and review every `warning`.
   - Only when a diagnostic has `suppressible: true` and the rule genuinely does not apply may you ignore it through `PatchBlock.lint_ignore`. Never use ignore to hide a real problem; core contract and safety rules are never suppressible.
   - `read_as_markdown(doc_id)` gives block anchors, flattened tables and expanded chart data for review.
   - On the delivery revision check digest `prose_char_count`, `target_prose_chars`, and `prose_char_delta` (actual minus agreed). If under the agreed length, add substantive analysis or disclose the gap; do not treat clean lint as meeting the target.
   - `GET /api/docs/{doc_id}` supplies canonical JSON plus `computed.table_layouts`, `computed.citation_map`, `computed.bibliography`, `computed.toc_tree` to the external frontend.

Rules:
- Do not invent IDs. On lookup failure use the `hint`/`valid_values` in the error.
- Put the visual main title in the outer block `title` ONLY. For multi-panel charts, `option.title` arrays may carry subplot labels; do not repeat the outer title verbatim inside `option.title` or `CustomHtml` headings.
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
  "style": { "keep_together": true }
}

DEFAULT COMPACT TABLE PRESET (8 columns or fewer)
- For 8 leaf columns or fewer, omit `style.column_widths` entirely and let the frontend Water-Filling allocator decide. It measures the same formatted cell text the renderer uses (including zero-padded decimals), locks short headers and enumerations to one line, keeps numeric cells unbreakable and retains a 78px floor for text columns during compression. Hand-written narrow percentages are the main cause of vertically squeezed short columns.
- Add explicit `column_widths` only when leaf columns >= 9 or when a preview confirms a specific locked ratio is required. Both percentages and pixels are accepted: `"style":{"column_widths":{"region":"24%","school":"136px"}}`.
- Keep leaf headers to 2–5 Chinese characters. Move parenthetical scopes and units (`（表述清晰度）`, `2026 快照，个`) into `note` or a grouped parent `ColumnNode`; do not repeat the same qualifier in every leaf header. For numeric `percent` columns where cells are raw numbers (`56.0`, `48.9`), include `（%）` in the leaf or parent column label (`青年占比（%）`) so the unit is self-explanatory.
- Keep status and enumeration values concise (`否（双一流）`, not a full sentence) so short columns can stay on one line.
- Prefer `keep_together: true` for compact benchmark tables.
Copyable 7-column preset (no manual widths):
{
  "id": "benchmark_2021",
  "type": "table",
  "title": "五校科研竞争力对标（2021年）",
  "numbering": "table",
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
  "rows": [
    { "school": "A校（本校）", "projects_2020": 190, "funding_2020": 15120, "projects_2021": 214, "funding_2021": 18650, "funding_rate": "18.4%", "growth_rate": "12.6%" }
  ],
  "style": { "keep_together": true }
}

Rules:
- Leaf columns require unique `key`. Group nodes may omit `key`.
- `data_type`: text | number | currency | percent | date | badge.
- `sizing`: fit (P90 content width) | grow (weighted share). Defaults: fit, weight 1.0.
- Alignment is inferred and can be overridden with `align` / `header_align`.
- Title rules: put the title in outer `title` ONLY (the alias `caption` is accepted for legacy input). Numbering is automatic (`表 N` / `图 N`); never hardcode it. `note` is an explanatory footnote.
- Compact shorthand is accepted: `{"headers":["Region","Revenue"],"rows":[["EMEA","$900"]],"summary_rows":[["Total","$900"]]}`. Column keys/types are inferred and array-form summary rows are normalized to row objects.
- The server pre-computes spans and normalized `<colgroup>` percentages in `computed.table_layouts`; frontends must not recalculate them.
- Table patch (in-place, preserves `block_id` and comment anchors): `{"type":"patch_block","block_id":"financials","patch":{"columns":[...],"rows":[...],"summary_rows":[...],"append_rows":[...],"update_cell":{"row_index":0,"column_key":"q1","value":1250000}}}`. Use `patch_block` with `columns` or `rows` instead of `replace_range` whenever renaming column headers or updating multiple rows on an existing table. The same `patch` object can set outer `title` (legacy alias `caption`), `numbering` and `note` for tables, charts and custom HTML.

ECHARTS BLOCK EXAMPLE (dual axis)
{
  "id": "revenue-chart",
  "type": "echarts",
  "title": "Revenue and margin",
  "numbering": "figure",
  "note": "Margins are unaudited.",
  "height": 360,
  "option": {
    "tooltip": { "trigger": "axis" },
    "xAxis": { "type": "category", "data": ["Q1", "Q2"] },
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
- Put the main title in outer `title` ONLY; never duplicate it inside ECharts `option.title`. A center donut KPI (`top: "center"`) is allowed, as are multi-panel subplot labels in an `option.title` array or a single positioned light subtitle (`left`/`right`/`bottom`, or `textStyle.fontSize <= 13`).
- `fallback_summary` is mandatory and must state the main numeric takeaway.
- `option` is the single, required source of truth: always send a complete native ECharts 6.0 option object.
- ECharts 6.0 supports `dataset` (2D arrays or object rows, multiple datasets), multi-axis Cartesian series, `matrix`, `radar`, `sankey`, `graph`, `chord`, `beeswarm`, `treemap`, `sunburst`, `tree`, `candlestick`, `boxplot`, `heatmap` and extension series such as `wordCloud`, `liquidFill` and `bar3D`. For deterministic multi-column ordering, prefer a two-dimensional `dataset.source` with a header row, or declare explicit `dimensions`; object-row key order is not guaranteed.
- Formatter strings are allowed and encouraged, for example `"formatter": "{b}: {c} ({d}%)"`. JavaScript callbacks (`function(` or `=>`) are forbidden; use ECharts formatter template strings instead.
- Static-report chrome is removed automatically: `toolbox` is stripped during normalization/normalization warnings. Do not design interactions that require it.
- Indexed series must stay in range: `xAxisIndex`, `yAxisIndex`, `gridIndex`, `datasetIndex`, `polarIndex` and `radarIndex` are validated against their configured arrays; axis-level `gridIndex` is validated too. A series with `coordinateSystem: "matrix"` requires a top-level `matrix` object.
- Matrix example:
  `{"matrix":{"x":{"data":["A","B"]},"y":{"data":["C","D"]}},"series":[{"type":"custom","coordinateSystem":"matrix","data":[1,2]}]}`.
  A series with `coordinateSystem: "matrix"` requires a top-level `matrix` object; radar/polar series require their matching top-level component.
- Every chart needs a real data source: `series[*].data`, `series[*].links`/`edges`, or a non-empty `dataset.source`, otherwise reviewers cannot verify the chart.
- Patch chart options with `{"type":"patch_block","patch":{"option":{...}}}` to replace the base option, or `option_patch` to deep-merge an incremental change. The same patch can update `height`, `title`, `numbering`, `note` and `fallback_summary`.
- `read_as_markdown(include_chart_data=true)` expands dataset, Cartesian, relation, radar and hierarchy data into tables for reviewers.

COMPOSITE / MULTI-PANEL CHARTS
- Use one ECharts block with a `grid` array for side-by-side panels or 2×2 small multiples. Recommended height: 360 for 1×2, 460–520 for 2×2. Do not create separate chart blocks when the panels share a caption, units or legend.
- Bind components explicitly. Each `xAxis[i]` / `yAxis[i]` sets `gridIndex` to its panel; each Cartesian series sets `xAxisIndex` / `yAxisIndex`; dataset-driven series also sets `datasetIndex`. Once more than one grid or axis exists, do not rely on implicit index 0.
- Subplot titles belong in an `option.title` array (or one positioned light subtitle). Keep the outer block `title` for the main figure title and never repeat it inside `option.title`.
- Left/right example (height 360):
  `"grid":[{"left":"4%","right":"54%","top":36,"bottom":24,"containLabel":true},{"left":"54%","right":"4%","top":36,"bottom":24,"containLabel":true}]`
  then `xAxis`/`yAxis` entries bind `gridIndex: 0` and `gridIndex: 1`, and series bind `xAxisIndex`/`yAxisIndex` to the matching panel. A right-hand pie may instead use `center` + `radius`; it needs no Cartesian axis.
- 2×2 small-multiples example (height 460–520):
  `"grid":[{"left":"6%","right":"52%","top":48,"bottom":"52%","containLabel":true},{"left":"52%","right":"6%","top":48,"bottom":"52%","containLabel":true},{"left":"6%","right":"52%","top":"56%","bottom":40,"containLabel":true},{"left":"52%","right":"6%","top":"56%","bottom":40,"containLabel":true}]`
  Provide four `xAxis`/`yAxis` entries with `gridIndex` 0–3 and one subtitle per panel, for example `"title":[{"text":"华东区","left":"6%","top":16,"textStyle":{"fontSize":12}}, ...]`.
- Cartesian and non-Cartesian series may share one option: pie uses `center`/`radius`, radar uses `radarIndex` and a `radar` component, and their grids can coexist with Cartesian panels. Every component still needs a real data source.
- Call `preview_echarts` and inspect the screenshot before commit: subplot titles must not collide with legends, axis names or labels, and each panel must keep readable margins at the committed height.
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
- Call `list_sources(doc_id)` to inspect every registered source, or `list_sources(doc_id, source_ids=[...])` for exact records and verbatim evidence excerpts. Never guess a source ID from its name.
- Search-result snippets are leads, not verified evidence. Read the original page and match a specific claim to the source before citing it; never attach a source only to satisfy a source-count target or clear a diagnostic.
- Citations: inline `[^sec10]`, or block-level `source_ids:["sec10"]`.
- Inline citations are numbered by first appearance. Bibliography order follows that numbering.
- `configure_references(doc_id, enabled=true, style="author_year")` switches labels to (Smith, 2025).
- Invalid source ids return the full valid source_id list in the diagnostic hint.

PAGE CHROME
- `configure_page_chrome(doc_id, header={"left":"{kicker}","center":"","right":"{title}"}, footer={"left":"{authors}","center":"第 {page} 页 / 共 {total} 页","right":"{date}"}, hide_header_on_first_page=true)`.
- Slots: header/footer each have `left`, `center`, `right`. Omit a slot object to keep its current value.
- Placeholders: `{page}`, `{total}`, `{title}`, `{subtitle}`, `{date}`, `{kicker}`, `{authors}`.
- First-page visibility: `hide_header_on_first_page` defaults true; `hide_footer_on_first_page` defaults false.
- Automatic heading page breaks: `heading_page_break="none"` (default, natural flow), `"h1"` (each level-1 heading starts a new page) or `"h1_and_h2"` (level-1 and level-2 headings start new pages). It never emits a blank page when the heading is already at the top of a fresh page.
- The frontend also exposes a visual editor for human reviewers; MCP values are the document defaults.
- A `cover.custom_html` cover always occupies its own first page in paged/print mode (header and footer hidden, content vertically centered); in continuous mode it keeps its natural height with a separator below.

THEME TOKENS
{
  "preset": "default",
  "tokens": {
    "--rpt-primary": "#1E3A5F",
    "--rpt-accent": "#3B6E9C",
    "--rpt-font-body": "Georgia, serif"
  }
}
- Token names must start with `--`; values cannot contain `; { } < > \`.
- Common tokens: --rpt-primary, --rpt-accent, --rpt-ink, --rpt-muted, --rpt-border, --rpt-surface, --rpt-background, --rpt-font-body, --rpt-font-heading, --rpt-radius, --rpt-content-width.

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

NOTES AND SOURCE ATTRIBUTION
- `table`, `echarts` and `custom_html` blocks support `note`: a footer note (statistical scope, units) rendered below the figure/table. It supports inline `[^source_id]`.
- Every block supports `source_note`: free-form source text rendered as `资料来源：...`, with inline `[^source_id]` support (for example `"根据教育部 2025 年公报[^moe2025]整理"`).
- `source_ids` attaches source records to the block; the frontend merges unreferenced IDs into an interactive citation badge such as `[1,2,3]`.
- Both `note` and `source_note` participate in citation numbering and the bibliography; use them instead of duplicating citation markers in body paragraphs.
"##.into(),
        recommended_tools: vec![
            "upsert_sources".into(),
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
- The Lead creates the report and chapter placeholder headings, records the user's hard requirements, and freezes verified facts separately from unresolved conflicts and hypotheses.
- For long multi-chapter reports, complete and review one pilot chapter before starting the remaining writers in parallel.
- Analysts own derived metrics, tables and charts. Writers own prose and disjoint block IDs, and return visual requests instead of adding duplicate visuals to satisfy lint.
- Reading is always allowed; mutations must use apply_operations with the latest expected_revision.
- Before delivery the Lead reads every chapter with read_as_markdown(section_id=heading_id). A reviewer's APPROVED applies only to the reviewed revision; any later content mutation requires another review.

SMART CAS
- Pass the revision you read. If another agent committed first, the server inspects intervening revisions.
- Disjoint block IDs -> automatic rebase; your batch commits at current_rev + 1.
- Receipts prove the rebase with `base_revision`, `rebased: true`, and `rebased_over: <count>`.
- Overlapping IDs -> REVISION_CONFLICT with `current_revision`, `conflicting_block_ids`, `conflicting_revisions`, and `latest_blocks`.
- Recovery: re-read digest/Markdown, patch the returned latest block (or re-send expected_revision=current_revision) and retry.
- A committed write can still have `blocking_errors`; these are lint errors scoped to blocks you touched. Fix them before declaring done.

ONLINE AUTHORING
- Every writer mutates the online ReportDocument through `apply_operations`; there is no offline file ingest.
- Replace a placeholder section with `{"type":"replace_range","start_id":"ch1_placeholder","end_id":"ch1_placeholder","blocks":[...]}` or target a heading with `append_to_section`.
- Empty or colliding block IDs are auto-assigned and returned in `assigned_ids`.
- Register sources in the same call with the inline `sources` map; URL/title deduplication commits atomically with the blocks.

REVIEW LOOP
1. `read_as_markdown(doc_id, section_id=heading_id)` for the section under review. Reuse the exact heading ID from the brief or digest (e.g. `ch1` or `c2-1`).
2. `lint_document(doc_id)`; fix every error and review warnings. Diagnostics carry `code`, `rule`, `hint` and `suppressible`; ignore only suppressible diagnostics that genuinely do not apply.
3. Patch specific blocks via block IDs from the Markdown anchors.
4. `rollback_revision(doc_id, target_revision)` creates a new monotonic revision; never rewrites history.

COMMENTS (SIDE-CHANNEL REVIEW)
- Review agents call `add_comment(doc_id, block_id, body, quote?)`; writers call `reply_comment(doc_id, thread_id, body, mark_addressed=true, addressed_rev=N)` after fixing the block.
- Comments do not increment `current_rev` and never participate in Smart CAS. A deleted block leaves the thread intact with `anchor_detached=true` and the original `anchor_snapshot`.
- `list_comments(doc_id, status?, block_id?)` defaults to open/addressed threads. Only the human REST PATCH endpoint may archive or reopen a thread; agents cannot send `archived`.

FRONTEND CONTRACT
- Frontend is read-only. It consumes GET /api/docs/{doc_id} canonical JSON + computed metadata and SSE revision deltas.
- GET /api/docs/{doc_id}/diff?fromRev=A&toRev=B returns structured block-level Added/Removed/Modified entries plus metadata flags.
- GET /api/docs/{doc_id}/sync?fromRev=A returns an applied snapshot delta through the current revision: exact target block order, only added/modified blocks, removed IDs, changed document configuration and canonical computed metadata. Read-only frontends use it for incremental live sync; the full document endpoint remains the initial-load and gap-recovery contract.
- SourceEntry.excerpt carries verbatim evidence; computed.chart_tables carries renderer-ready ECharts data tables grouped by block ID.
- SSE events: revision, ephemeral_preview, agent_presence, ephemeral_clear, comment_updated, sync_required, plus heartbeat comments.
- Do not send cursor/presence edits other than optional ephemeral previews. When a previewed block is committed, pass its preview IDs as preview_ids to apply_operations so revision.cleared_preview_ids clears the placeholder atomically. A no-op commit emits ephemeral_clear immediately instead of creating a revision.
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
