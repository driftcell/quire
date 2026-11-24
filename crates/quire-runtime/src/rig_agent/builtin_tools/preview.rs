use super::*;

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
    let result = crate::preview::execute_preview_report(&runtime.engine.config, input)
        .await
        .map_err(tool_other)?;
    let mut content = vec![ToolResultContent::json(
        result.to_diagnostics_json().map_err(tool_other)?,
    )];
    if let (Some(base64), Some(mime_type)) = (result.image_base64, result.image_mime_type) {
        content.push(ToolResultContent::text(REPORT_PREVIEW_CAPTION));
        content.push(ToolResultContent::image_base64(
            base64,
            Some(image_media_type(&mime_type)),
            None,
        ));
    }
    ToolOutput::content(content)
}

/// Validate and render an ECharts 6.0 chart in headless Chromium before committing it to the report.
///
/// - First call: supply the complete native ECharts `option` alongside outer metadata (`title`, `note`,
///   `fallback_summary`, `height`, `source_ids`, `source_note`).
/// - Refinement calls: supply `based_on_preview_id` and a minimal `option_patch` (never resend `option`,
///   or the call will fail).
/// - Receipt: returns `preview_id` (valid for 30 minutes), `lint_issues`, `preview_url`, and a PNG
///   screenshot for vision-capable models. Resolve all errors and visual overlaps before calling `commit_chart`.
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
    /// Concise chart caption/note (`<= 120` characters) stating measurement units and scope.
    note: Option<String>,
    /// Required one-sentence quantitative takeaway stating extremes, deltas, or trends (missing summary triggers a lint warning).
    fallback_summary: Option<String>,
    /// Registered canonical `source_id`s persisted with the preview so `commit_chart` inherits attribution automatically.
    source_ids: Option<Vec<String>>,
    /// Optional source attribution note persisted with the preview and inherited by `commit_chart`.
    source_note: Option<String>,
) -> Result<ToolOutput, ToolExecutionError> {
    let runtime = runtime_tool_scope(context)?;
    let turn = quire_turn_meta(context)?;
    let config = runtime.engine.config.as_ref();
    let bus = runtime.engine.kb.store().bus();
    let based_on = based_on_preview_id
        .as_deref()
        .map(str::trim)
        .filter(|value| !value.is_empty());

    let mut based_on_id = None;
    let mut effective_source_ids: Vec<String> = source_ids
        .clone()
        .unwrap_or_default()
        .into_iter()
        .map(|s| s.trim().to_string())
        .filter(|s| !s.is_empty())
        .collect();
    let mut effective_source_note = normalize_optional_text(source_note.clone());
    let chart = if let Some(base_id) = based_on {
        if option.is_some() {
            return Err(ToolExecutionError::invalid_args(
                "`option` must not be sent when `based_on_preview_id` is set; send only `option_patch` so the full ECharts configuration is not repeated",
            ));
        }
        let existing = bus
            .load_chart_preview(base_id)
            .await
            .map_err(tool_other)?
            .ok_or_else(|| {
                ToolExecutionError::invalid_args(format!(
                    "chart preview `{base_id}` was not found or expired after 30 minutes; call preview_echarts again with the complete native `option`"
                ))
            })?;
        based_on_id = Some(base_id.to_string());
        let mut chart = existing.chart;
        let mut changed = false;
        if let Some(patch) = &option_patch {
            if !patch.is_object() {
                return Err(ToolExecutionError::invalid_args(
                    "`option_patch` must be a JSON object",
                ));
            }
            chart.option_patch = Some(merge_chart_option_patch(chart.option_patch.as_ref(), patch));
            changed = true;
        }
        if let Some(height) = height {
            chart.height = height;
            changed = true;
        }
        if title.is_some() {
            chart.title = normalize_optional_text(title);
            changed = true;
        }
        if note.is_some() {
            chart.note = normalize_optional_text(note);
            changed = true;
        }
        if let Some(summary) = fallback_summary {
            chart.fallback_summary = summary;
            changed = true;
        }
        if source_ids.is_some() {
            changed = true;
        } else {
            effective_source_ids = existing.source_ids;
        }
        if source_note.is_some() {
            changed = true;
        } else {
            effective_source_note = existing.source_note;
        }
        if !changed {
            return Err(ToolExecutionError::invalid_args(
                "iterative preview requires `option_patch` or at least one top-level override",
            ));
        }
        chart
    } else {
        let option = option.ok_or_else(|| {
            ToolExecutionError::invalid_args(
                "`option` is required on the first preview_echarts call; send the complete native ECharts 6.0 option",
            )
        })?;
        if !option.is_object() {
            return Err(ToolExecutionError::invalid_args(
                "`option` must be a JSON object",
            ));
        }
        if let Some(patch) = &option_patch
            && !patch.is_object()
        {
            return Err(ToolExecutionError::invalid_args(
                "`option_patch` must be a JSON object",
            ));
        }
        EchartsBlock {
            title: normalize_optional_text(title),
            height: height.unwrap_or(360),
            option,
            option_patch,
            fallback_summary: fallback_summary.unwrap_or_default(),
            numbering: None,
            note: normalize_optional_text(note),
        }
    };

    let preview_id = short_id("chart");
    let preview_block_id = BlockId::parse("chart_preview").map_err(tool_other)?;
    let mut lint = lint_echarts_block(preview_block_id, &chart);
    let has_preview_source = !effective_source_ids.is_empty()
        || effective_source_note.is_some()
        || chart.note.as_deref().is_some_and(|n| n.contains("[^"));
    if has_preview_source {
        lint.diagnostics.retain(|d| d.code != "QR422");
        lint.summary.warnings = lint
            .diagnostics
            .iter()
            .filter(|d| d.severity == quire_protocol::Severity::Warning)
            .count();
    }
    let requirements = chart_requirements(&chart);
    let tables = chart_data_tables_for_block(&chart);
    let record = crate::preview::ChartPreviewRecord {
        preview_id: preview_id.clone(),
        title: chart.title.clone(),
        note: chart.note.clone(),
        chart,
        requirements,
        tables,
        based_on_preview_id: based_on_id,
        source_ids: effective_source_ids,
        source_note: effective_source_note,
    };
    bus.store_chart_preview(&record).await.map_err(tool_other)?;

    let mut preview_url = crate::preview::build_public_chart_preview_url(config, &preview_id);
    let mut preview_error = None;
    let mut image_included = false;
    let mut preview_image = None;
    if turn.supports_vision {
        match crate::preview::execute_preview_echarts(config, &preview_id).await {
            Ok(result) => {
                preview_url = result.preview_url;
                if let (Some(base64), Some(mime_type)) =
                    (result.image_base64, result.image_mime_type)
                {
                    preview_image = Some((base64, mime_type));
                    image_included = true;
                }
            }
            Err(error) => preview_error = Some(error.to_string()),
        }
    }

    let lint_issues = lint.diagnostics;
    let lint_summary = lint.summary;
    let diagnostics = serde_json::to_value(PreviewEchartsReceipt {
        preview_id,
        preview_url,
        lint_issues,
        lint_summary,
        image_included,
        preview_error,
    })
    .map_err(tool_other)?;
    let mut content = vec![ToolResultContent::json(diagnostics)];
    if let Some((base64, mime_type)) = preview_image {
        content.push(ToolResultContent::text(CHART_PREVIEW_CAPTION));
        content.push(ToolResultContent::image_base64(
            base64,
            Some(image_media_type(&mime_type)),
            None,
        ));
    }
    ToolOutput::content(content)
}

fn normalize_optional_text(value: Option<String>) -> Option<String> {
    value
        .map(|text| text.trim().to_string())
        .filter(|text| !text.is_empty())
}

/// Commit a chart that already passed `preview_echarts` into the report using its cached `preview_id`
/// (never resend the ECharts `option`).
///
/// If `block_id` already exists as an `"echarts"` block in `doc_id`, updates it in place via
/// `PatchBlock` (preserving review comment anchors) and repositions it when `after_block_id` or
/// `before_block_id` is supplied. Automatically inherits `source_ids` and `source_note` from the
/// cached preview unless overridden.
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
) -> Result<CommitChartReceipt, ToolExecutionError> {
    let runtime = runtime_tool_scope(context)?;
    let req_ctx = in_process_request_context(context)?;
    let preview_id = preview_id.trim().to_string();
    if preview_id.is_empty() {
        return Err(ToolExecutionError::invalid_args(
            "`preview_id` must not be empty",
        ));
    }
    let raw_doc_id = doc_id.unwrap_or_default();
    let resolved_doc_id = resolve_existing_session_doc_id(context, &runtime, &raw_doc_id).await?;
    let preview = runtime
        .engine
        .kb
        .store()
        .bus()
        .load_chart_preview(&preview_id)
        .await
        .map_err(tool_other)?
        .ok_or_else(|| {
            ToolExecutionError::invalid_args(format!(
                "chart preview `{preview_id}` was not found or expired after 30 minutes; call preview_echarts again to regenerate it"
            ))
        })?;
    let Some(doc_id) = resolved_doc_id else {
        // Pure chat-session mode: alias the cached preview under `block_id` in Redis
        // so `<echarts id="block_id"/>` and `<echarts id="preview_id"/>` both render
        // in chat without creating or linking a report document.
        let target_id = BlockId::parse(block_id.trim()).map_err(tool_other)?;
        let mut aliased = preview.clone();
        aliased.preview_id = target_id.as_str().to_string();
        if let Some(override_ids) = source_ids {
            aliased.source_ids = override_ids;
        }
        if let Some(override_note) = normalize_optional_text(source_note) {
            aliased.source_note = Some(override_note);
        }
        runtime
            .engine
            .kb
            .store()
            .bus()
            .store_chart_preview(&aliased)
            .await
            .map_err(tool_other)?;
        return Ok(CommitChartReceipt {
            mutation: quire_server::mcp::ApplyOperationsReceipt {
                new_revision: 0,
                base_revision: 0,
                rebased: false,
                rebased_over: 0,
                committed: true,
                assigned_ids: Vec::new(),
                inline_warnings: Vec::new(),
                blocking_errors: Vec::new(),
                _next_hint: format!(
                    "Chart `{}` is ready for inline chat rendering via `<echarts id=\"{}\"/>` (no report document created).",
                    target_id.as_str(),
                    target_id.as_str()
                ),
            },
            preview_id,
            block_id: target_id.as_str().to_string(),
        });
    };
    let after_block_id = normalize_optional_text(after_block_id);
    let before_block_id = normalize_optional_text(before_block_id);
    let section_id = normalize_optional_text(section_id);
    if after_block_id.is_some() && before_block_id.is_some() {
        return Err(ToolExecutionError::invalid_args(
            "use at most one of `after_block_id` or `before_block_id`",
        ));
    }
    if !preview.chart.option.is_object() {
        return Err(ToolExecutionError::invalid_args(
            "stored chart preview has no native ECharts `option`; call preview_echarts again",
        ));
    }

    let target_id = BlockId::parse(block_id.trim()).map_err(tool_other)?;
    let raw_source_ids = source_ids.unwrap_or(preview.source_ids);
    let mut parsed_source_ids = Vec::with_capacity(raw_source_ids.len());
    for raw_id in raw_source_ids {
        let trimmed = raw_id.trim();
        if !trimmed.is_empty() {
            parsed_source_ids.push(quire_protocol::SourceId::parse(trimmed).map_err(tool_other)?);
        }
    }
    let effective_source_note = normalize_optional_text(source_note).or(preview.source_note);

    let rmcp::handler::server::wrapper::Json(digest_receipt) = runtime
        .report_mcp
        .get_document_digest(rmcp::handler::server::wrapper::Parameters(
            quire_server::mcp::DigestParams {
                doc_id: doc_id.clone(),
                query: None,
                target_prose_chars: None,
            },
        ))
        .await
        .map_err(mcp_error_to_tool_error)?;
    let existing_echarts = digest_receipt
        .digest
        .blocks
        .iter()
        .any(|b| b.id == target_id.as_str() && b.block_type == "echarts");

    let expected_revision = match expected_revision {
        Some(value) if value >= 0 => value,
        Some(_) => {
            return Err(ToolExecutionError::invalid_args(
                "`expected_revision` must be non-negative",
            ));
        }
        None => digest_receipt.digest.current_rev,
    };

    let ops = if existing_echarts {
        let chart = preview.chart;
        let patch = quire_protocol::BlockPatch {
            option: Some(chart.option),
            option_patch: chart.option_patch,
            height: Some(chart.height),
            fallback_summary: Some(chart.fallback_summary),
            title: chart.title,
            note: chart.note,
            source_ids: (!parsed_source_ids.is_empty()).then_some(parsed_source_ids),
            source_note: effective_source_note,
            ..Default::default()
        };
        let mut patch_ops = vec![PatchOp::PatchBlock {
            block_id: target_id.clone(),
            patch: Box::new(patch),
        }];
        if let Some(anchor) = after_block_id {
            let anchor_id = BlockId::parse(anchor.trim()).map_err(tool_other)?;
            if anchor_id != target_id {
                patch_ops.push(PatchOp::MoveBlock {
                    block_id: target_id.clone(),
                    after_id: Some(anchor_id),
                });
            }
        } else if let Some(anchor) = before_block_id {
            let anchor_id = BlockId::parse(anchor.trim()).map_err(tool_other)?;
            if anchor_id != target_id {
                let block_ids: Vec<&str> = digest_receipt
                    .digest
                    .blocks
                    .iter()
                    .map(|block| block.id.as_str())
                    .collect();
                let target_index = block_ids.iter().position(|id| *id == target_id.as_str());
                let anchor_index = block_ids
                    .iter()
                    .position(|id| *id == anchor_id.as_str())
                    .ok_or_else(|| {
                        ToolExecutionError::invalid_args(format!(
                            "`before_block_id` anchor `{}` does not exist",
                            anchor_id.as_str()
                        ))
                    })?;
                let already_before = target_index.is_some_and(|index| index + 1 == anchor_index);
                if !already_before {
                    let predecessor = anchor_index
                        .checked_sub(1)
                        .and_then(|index| block_ids.get(index))
                        .and_then(|id| (*id != target_id.as_str()).then_some(*id))
                        .map(|id| BlockId::parse(id).map_err(tool_other))
                        .transpose()?;
                    patch_ops.push(PatchOp::MoveBlock {
                        block_id: target_id.clone(),
                        after_id: predecessor,
                    });
                }
            }
        }
        patch_ops
    } else {
        let mut block = Block::new(target_id.clone(), BlockKind::Echarts(preview.chart));
        block.source_ids = parsed_source_ids;
        block.source_note = effective_source_note;
        let op = if let Some(anchor) = after_block_id {
            PatchOp::InsertAfter {
                target_id: BlockId::parse(anchor.trim()).map_err(tool_other)?,
                blocks: vec![block],
            }
        } else if let Some(anchor) = before_block_id {
            PatchOp::InsertBefore {
                target_id: BlockId::parse(anchor.trim()).map_err(tool_other)?,
                blocks: vec![block],
            }
        } else if let Some(section) = section_id {
            PatchOp::AppendToSection {
                section_id: BlockId::parse(section.trim()).map_err(tool_other)?,
                blocks: vec![block],
            }
        } else {
            PatchOp::Append {
                blocks: vec![block],
            }
        };
        vec![op]
    };

    let summary = summary
        .map(|value| value.trim().to_string())
        .filter(|value| !value.is_empty())
        .unwrap_or_else(|| format!("commit previewed chart `{target_id}`"));
    let rmcp::handler::server::wrapper::Json(receipt) = runtime
        .report_mcp
        .exec_apply_operations(
            req_ctx,
            quire_server::mcp::ApplyOperationsParams {
                doc_id,
                expected_revision,
                summary,
                ops,
                sources: None,
                preview_ids: Vec::new(),
            },
        )
        .await
        .map_err(mcp_error_to_tool_error)?;
    Ok(CommitChartReceipt {
        mutation: receipt,
        preview_id,
        block_id: target_id.to_string(),
    })
}
