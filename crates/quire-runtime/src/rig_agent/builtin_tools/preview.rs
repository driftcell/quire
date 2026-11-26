use super::*;

fn preview_service(runtime: &RuntimeToolScope) -> crate::preview::PreviewToolService {
    crate::preview::PreviewToolService::new(
        runtime.engine.config.clone(),
        runtime.engine.kb.store().clone(),
        runtime.report.clone(),
    )
}

fn into_rig_output(
    result: quire_server::state::ExternalPreviewToolResult,
) -> Result<ToolOutput, ToolExecutionError> {
    let mut content = vec![ToolResultContent::json(result.payload)];
    if let Some(image) = result.image {
        content.push(ToolResultContent::text(image.caption));
        content.push(ToolResultContent::image_base64(
            image.base64,
            Some(image_media_type(&image.mime_type)),
            None,
        ));
    }
    ToolOutput::content(content)
}

/// Render a committed report in headless Chromium and return A4 pagination metrics,
/// horizontal overflow diagnostics (`layout_diagnostics.overflow_count` and `overflow_elements`),
/// computed CSS styles, and page screenshots for vision-capable models.
///
/// Pass `block_ids` or `section_id` for fast partial inspection: partial previews automatically
/// suppress the cover, TOC, and bibliography so Page 1 directly renders the target table, chart,
/// or section. Omit both for a full-document preview.
#[expect(
    clippy::too_many_arguments,
    reason = "rig_tool exposes each argument as a separate JSON Schema field"
)]
#[rig_tool(name = "preview_report")]
pub(crate) async fn preview_report_tool(
    #[rig(context)] context: &mut ToolContext,
    /// Target report document ID.
    doc_id: String,
    /// Layout mode: `"paged"` (default A4 pagination) or `"continuous"` (single-column scroll).
    mode: Option<crate::preview::PreviewViewMode>,
    /// Heading `block_id` (e.g. `"ch2"`) to preview only that section. Automatically hides cover, TOC, and references.
    section_id: Option<String>,
    /// Specific block IDs (e.g. `["tbl_peers"]`) to preview in isolation. Automatically hides cover, TOC, and references.
    block_ids: Option<Vec<String>>,
    /// 1-based A4 page number filter when `mode` is `"paged"`.
    page: Option<usize>,
    /// Exact committed revision (`>= 1`) to assert; omit to inspect the latest committed revision.
    rev: Option<u64>,
    /// Include active theme tokens, `custom_css`, and browser computed styles (defaults to `true`).
    include_styles: Option<bool>,
) -> Result<ToolOutput, ToolExecutionError> {
    let runtime = runtime_tool_scope(context)?;
    let turn = quire_turn_meta(context)?;
    let request_context = in_process_request_context(context)?;
    let input = crate::preview::PreviewReportInput {
        doc_id,
        mode: mode.unwrap_or_default(),
        section_id,
        block_ids,
        page,
        rev,
        include_styles: include_styles.unwrap_or(true),
        include_image: turn.supports_vision,
    };
    let result = preview_service(&runtime)
        .preview_report(request_context, input)
        .await
        .map_err(tool_other)?;
    into_rig_output(result)
}

/// Validate and render an ECharts 6.0 chart in headless Chromium before committing it to the report.
///
/// - First call: supply the complete native ECharts `option` alongside outer metadata (`title`, `note`,
///   `fallback_summary`, `height`, `source_ids`, `source_note`).
/// - Refinement calls: supply `based_on_preview_id` and a minimal `option_patch` (never resend `option`,
///   or the call will fail).
/// - Receipt: returns `preview_id` (valid for 30 minutes), `lint_issues`, `preview_url`, and a PNG
///   screenshot for vision-capable models. Resolve all errors and visual overlaps before calling `commit_chart`.
/// - In chat replies, embed `<echarts id="PREVIEW_ID"/>` on its own line to render the interactive chart preview.
#[expect(
    clippy::too_many_arguments,
    reason = "rig_tool exposes each argument as a separate JSON Schema field"
)]
#[rig_tool(name = "preview_echarts")]
pub(crate) async fn preview_echarts_tool(
    #[rig(context)] context: &mut ToolContext,
    /// Complete native ECharts 6.0 option JSON object on the first call (no JS functions `function(` or `=>`). Must be omitted when `based_on_preview_id` is set.
    option: Option<Value>,
    /// Minimal incremental JSON patch applied on top of `based_on_preview_id` (arrays like `series`/`xAxis`/`yAxis`/`grid` merge element-by-element).
    option_patch: Option<Value>,
    /// Previous `preview_id` from `preview_echarts` (cached for 30 minutes) to refine incrementally without resending `option`.
    based_on_preview_id: Option<String>,
    /// Chart canvas height in pixels (defaults to `360`; use `460..520` for 2x2 multi-panel grids).
    height: Option<u32>,
    /// Outer chart block title. Do not duplicate the main title inside `option.title`.
    title: Option<String>,
    /// Concise Chinese caption (`<= 120` chars) stating units and statistical scope.
    /// Good: `"单位：项；统计范围为2021—2025年集中接收期立项数。"`; Bad: `"如图所示"`.
    note: Option<String>,
    /// Required one-sentence Chinese quantitative takeaway stating subject, extremes, delta, or trend.
    /// Good: `"2021—2025年浙江工业大学国自然立项由142项增至198项（+39.4%），增速居省属高校首位。"`; Bad: `"展示了各高校立项趋势变化。"`.
    fallback_summary: Option<String>,
    /// Registered canonical `source_id`s persisted with the preview so `commit_chart` inherits attribution automatically.
    source_ids: Option<Vec<String>>,
    /// Optional source attribution note persisted with the preview and inherited by `commit_chart`.
    source_note: Option<String>,
) -> Result<ToolOutput, ToolExecutionError> {
    let runtime = runtime_tool_scope(context)?;
    let turn = quire_turn_meta(context)?;
    let request = quire_server::report::PreviewEchartsParams {
        option,
        option_patch,
        based_on_preview_id,
        height,
        title,
        note,
        fallback_summary,
        source_ids,
        source_note,
        include_image: turn.supports_vision,
    };
    let result = preview_service(&runtime)
        .preview_echarts(in_process_request_context(context)?, request)
        .await
        .map_err(tool_other)?;
    into_rig_output(result)
}

/// Commit a chart that already passed `preview_echarts` into the report using its cached `preview_id`
/// (never resend the ECharts `option`).
///
/// If `block_id` already exists as an `"echarts"` block in `doc_id`, updates it in place via
/// `PatchBlock` (preserving review comment anchors) and repositions it when `after_block_id` or
/// `before_block_id` is supplied. Automatically inherits `source_ids` and `source_note` from the
/// cached preview unless overridden. In chat replies, embed `<echarts id="BLOCK_ID"/>` on its own
/// line to render the committed interactive chart.
#[expect(
    clippy::too_many_arguments,
    reason = "rig_tool exposes each argument as a separate JSON Schema field"
)]
#[rig_tool(name = "commit_chart")]
pub(crate) async fn commit_chart_tool(
    #[rig(context)] context: &mut ToolContext,
    /// Cached `preview_id` returned by `preview_echarts` (valid for 30 minutes).
    preview_id: String,
    /// Target report document ID (optional; if omitted, uses or auto-creates the session's bound document so the chart can also be rendered in chat via `<echarts id="..."/>`).
    doc_id: Option<String>,
    /// Short semantic block ID for the chart (e.g. `"ch2_funding_chart"`). Updates in place if an ECharts block with this ID already exists.
    block_id: String,
    /// Insert or move the chart immediately after this existing `block_id` (mutually exclusive with `before_block_id`).
    after_block_id: Option<String>,
    /// Insert or move the chart immediately before this existing `block_id` (mutually exclusive with `after_block_id`).
    before_block_id: Option<String>,
    /// Append a new chart to the end of this Heading `block_id` section when neither `after_block_id` nor `before_block_id` is set.
    section_id: Option<String>,
    /// Base revision for Smart CAS rebasing; omit to auto-fetch the current revision before committing.
    expected_revision: Option<i64>,
    /// Short commit summary describing the chart insertion or update.
    summary: Option<String>,
    /// Optional canonical `source_id`s overriding those stored during `preview_echarts`.
    source_ids: Option<Vec<String>>,
    /// Optional source attribution note overriding the one stored during `preview_echarts`.
    source_note: Option<String>,
    /// Where the chart will be used: `"document"` (default; commits into `doc_id` when a report is bound) or `"chat"` (persists the chart for inline chat rendering via `<echarts id="..."/>` without modifying the report or triggering report lint).
    scope: Option<super::report_inproc::TargetScope>,
) -> Result<crate::preview::CommitChartReceipt, ToolExecutionError> {
    let runtime = runtime_tool_scope(context)?;
    let request_context = in_process_request_context(context)?;
    let scope = scope.unwrap_or_default();
    let doc_id = if scope == super::report_inproc::TargetScope::Chat {
        None
    } else {
        resolve_existing_session_doc_id(context, &runtime, doc_id.as_deref().unwrap_or_default())
            .await?
    };
    let request = quire_server::report::CommitChartParams {
        preview_id,
        doc_id,
        block_id,
        after_block_id,
        before_block_id,
        section_id,
        expected_revision,
        summary,
        source_ids,
        source_note,
        scope: Some(scope),
    };
    preview_service(&runtime)
        .commit_chart(request_context, request)
        .await
        .map_err(tool_other)
}
