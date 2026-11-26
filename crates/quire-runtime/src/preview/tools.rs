use std::sync::Arc;

use quire_lint::lint_echarts_block;
use quire_protocol::{
    Block, BlockId, BlockKind, BlockPatch, Diagnostic, DocId, EchartsBlock, LintSummary, PatchOp,
    SessionId, SourceId, chart_data_tables_for_block, chart_requirements, merge_chart_option_patch,
};
use quire_server::{
    auth::RequestContext,
    report::{
        CommitChartParams, PreviewEchartsParams, PreviewReportInput, QuireReportService,
        TargetScope,
    },
    state::{ExternalPreviewImage, ExternalPreviewToolResult},
};
use schemars::JsonSchema;
use serde::Serialize;

use crate::{
    config::Config,
    error::{RuntimeError, RuntimeResult},
    store::RuntimeStore,
};

const REPORT_PREVIEW_CAPTION: &str = "Rendered screenshot from preview_report. Inspect visual layout, spacing, tables, and charts alongside the diagnostic JSON.";
const CHART_PREVIEW_CAPTION: &str = "Rendered ECharts screenshot from preview_echarts. Verify the chart type, series data, axis labels, legend, tooltip/static labels, colors, clipping/overflow, and that the visual matches fallback_summary. Refine with preview_echarts + based_on_preview_id + option_patch only, then call commit_chart once it looks right.";

#[derive(Debug, Serialize, JsonSchema)]
pub struct PreviewEchartsReceipt {
    pub preview_id: String,
    pub preview_url: String,
    pub lint_issues: Vec<Diagnostic>,
    pub lint_summary: LintSummary,
    pub image_included: bool,
    pub preview_error: Option<String>,
}

#[derive(Debug, Serialize, JsonSchema)]
pub struct CommitChartReceipt {
    #[serde(flatten)]
    pub mutation: quire_server::report::ApplyOperationsReceipt,
    pub preview_id: String,
    pub block_id: String,
}

/// Runtime-owned preview execution shared by native Rig tools and external rmcp adapters.
#[derive(Clone)]
pub struct PreviewToolService {
    config: Arc<Config>,
    store: RuntimeStore,
    report: QuireReportService,
}

impl PreviewToolService {
    pub fn new(config: Arc<Config>, store: RuntimeStore, report: QuireReportService) -> Self {
        Self {
            config,
            store,
            report,
        }
    }

    pub async fn preview_report(
        &self,
        context: RequestContext,
        request: PreviewReportInput,
    ) -> RuntimeResult<ExternalPreviewToolResult> {
        let doc_id = DocId::parse(request.doc_id.trim())
            .map_err(|error| RuntimeError::InvalidRequest(error.to_string()))?;
        self.report
            .state()
            .authorize_doc_access(&context, &doc_id, false)
            .await
            .map_err(store_error)?;
        let result = super::execute_preview_report(&self.config, request).await?;
        let payload = result.to_diagnostics_json()?;
        let image = match (result.image_base64, result.image_mime_type) {
            (Some(base64), Some(mime_type)) => Some(ExternalPreviewImage {
                base64,
                mime_type,
                caption: REPORT_PREVIEW_CAPTION.to_string(),
            }),
            _ => None,
        };
        Ok(ExternalPreviewToolResult { payload, image })
    }

    async fn authorize_session(&self, context: &RequestContext) -> RuntimeResult<SessionId> {
        let session_id = context.session_id.clone().ok_or_else(|| {
            RuntimeError::InvalidRequest(
                "preview tools require X-Session-Id or _meta.quire.session_id".into(),
            )
        })?;
        self.report
            .state()
            .authorize_session_access(context, &session_id)
            .await
            .map_err(store_error)?;
        Ok(session_id)
    }

    async fn load_owned_preview(
        &self,
        context: &RequestContext,
        session_id: &SessionId,
        preview_id: &str,
    ) -> RuntimeResult<super::ChartPreviewRecord> {
        let preview = self
            .store
            .bus()
            .load_chart_preview(preview_id)
            .await?
            .ok_or_else(|| RuntimeError::ChartPreviewNotFound(preview_id.to_string()))?;
        if preview.owner_user_id.as_deref() != context.user_id.as_deref()
            || preview.session_id.as_deref() != Some(session_id.as_str())
        {
            return Err(RuntimeError::Forbidden(
                "chart preview belongs to another user or session; create a fresh preview".into(),
            ));
        }
        Ok(preview)
    }

    pub async fn preview_echarts(
        &self,
        context: RequestContext,
        request: PreviewEchartsParams,
    ) -> RuntimeResult<ExternalPreviewToolResult> {
        let session_id = self.authorize_session(&context).await?;
        let PreviewEchartsParams {
            option,
            option_patch,
            based_on_preview_id,
            height,
            title,
            note,
            fallback_summary,
            source_ids,
            source_note,
            include_image,
        } = request;
        let based_on = based_on_preview_id
            .as_deref()
            .map(str::trim)
            .filter(|value| !value.is_empty());

        let mut based_on_id = None;
        let mut effective_source_ids: Vec<String> = source_ids
            .clone()
            .unwrap_or_default()
            .into_iter()
            .map(|value| value.trim().to_string())
            .filter(|value| !value.is_empty())
            .collect();
        let mut effective_source_note = normalize_optional_text(source_note.clone());
        let chart = if let Some(base_id) = based_on {
            if option.is_some() {
                return Err(RuntimeError::InvalidRequest(
                    "`option` must not be sent when `based_on_preview_id` is set; send only `option_patch`"
                        .into(),
                ));
            }
            let existing = self
                .load_owned_preview(&context, &session_id, base_id)
                .await?;
            based_on_id = Some(base_id.to_string());
            let mut chart = existing.chart;
            let mut changed = false;
            if let Some(patch) = &option_patch {
                if !patch.is_object() {
                    return Err(RuntimeError::InvalidRequest(
                        "`option_patch` must be a JSON object".into(),
                    ));
                }
                chart.option_patch =
                    Some(merge_chart_option_patch(chart.option_patch.as_ref(), patch));
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
                return Err(RuntimeError::InvalidRequest(
                    "iterative preview requires `option_patch` or at least one top-level override"
                        .into(),
                ));
            }
            chart
        } else {
            let option = option.ok_or_else(|| {
                RuntimeError::InvalidRequest(
                    "`option` is required on the first preview_echarts call; send the complete native ECharts 6.0 option"
                        .into(),
                )
            })?;
            if !option.is_object() {
                return Err(RuntimeError::InvalidRequest(
                    "`option` must be a JSON object".into(),
                ));
            }
            if let Some(patch) = &option_patch
                && !patch.is_object()
            {
                return Err(RuntimeError::InvalidRequest(
                    "`option_patch` must be a JSON object".into(),
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

        let preview_id = quire_protocol::compact_id("chart");
        let preview_block_id = BlockId::parse("chart_preview")
            .map_err(|error| RuntimeError::InvalidRequest(error.to_string()))?;
        let mut lint = lint_echarts_block(preview_block_id, &chart);
        let has_preview_source = !effective_source_ids.is_empty()
            || effective_source_note.is_some()
            || chart
                .note
                .as_deref()
                .is_some_and(|note| note.contains("[^"));
        if has_preview_source {
            lint.diagnostics
                .retain(|diagnostic| diagnostic.code != "QR422");
            lint.summary.warnings = lint
                .diagnostics
                .iter()
                .filter(|diagnostic| diagnostic.severity == quire_protocol::Severity::Warning)
                .count();
        }
        let requirements = chart_requirements(&chart);
        let tables = chart_data_tables_for_block(&chart);
        let record = super::ChartPreviewRecord {
            preview_id: preview_id.clone(),
            title: chart.title.clone(),
            note: chart.note.clone(),
            chart,
            requirements,
            tables,
            based_on_preview_id: based_on_id,
            source_ids: effective_source_ids,
            source_note: effective_source_note,
            owner_user_id: context.user_id.clone(),
            session_id: Some(session_id.as_str().to_string()),
        };
        self.store.bus().store_chart_preview(&record).await?;

        let mut preview_url = super::build_public_chart_preview_url(&self.config, &preview_id);
        let mut preview_error = None;
        let mut image_included = false;
        let mut preview_image = None;
        if include_image {
            match super::execute_preview_echarts(&self.config, &preview_id).await {
                Ok(result) => {
                    preview_url = result.preview_url;
                    if let (Some(base64), Some(mime_type)) =
                        (result.image_base64, result.image_mime_type)
                    {
                        preview_image = Some(ExternalPreviewImage {
                            base64,
                            mime_type,
                            caption: CHART_PREVIEW_CAPTION.to_string(),
                        });
                        image_included = true;
                    }
                }
                Err(error) => preview_error = Some(error.to_string()),
            }
        }

        let payload = serde_json::to_value(PreviewEchartsReceipt {
            preview_id,
            preview_url,
            lint_issues: lint.diagnostics,
            lint_summary: lint.summary,
            image_included,
            preview_error,
        })?;
        Ok(ExternalPreviewToolResult {
            payload,
            image: preview_image,
        })
    }

    pub async fn commit_chart(
        &self,
        context: RequestContext,
        request: CommitChartParams,
    ) -> RuntimeResult<CommitChartReceipt> {
        let session_id = self.authorize_session(&context).await?;
        let preview_id = request.preview_id.trim().to_string();
        if preview_id.is_empty() {
            return Err(RuntimeError::InvalidRequest(
                "`preview_id` must not be empty".into(),
            ));
        }
        let preview = self
            .load_owned_preview(&context, &session_id, &preview_id)
            .await?;
        let scope = request.scope.unwrap_or_default();
        let raw_doc_id = request.doc_id.unwrap_or_default();
        let resolved_doc_id = if scope == TargetScope::Chat {
            None
        } else if raw_doc_id.trim().is_empty() {
            let summary = self.store.get_session_summary(session_id.as_str()).await?;
            summary.doc_id.or(summary.linked_doc_id)
        } else {
            Some(raw_doc_id)
        };

        let Some(doc_id) = resolved_doc_id else {
            let target_id = BlockId::parse(request.block_id.trim())
                .map_err(|error| RuntimeError::InvalidRequest(error.to_string()))?;
            let mut aliased = preview.clone();
            aliased.preview_id = target_id.as_str().to_string();
            if let Some(override_ids) = request.source_ids {
                aliased.source_ids = override_ids;
            }
            if let Some(override_note) = normalize_optional_text(request.source_note) {
                aliased.source_note = Some(override_note);
            }
            self.store
                .save_chat_chart(session_id.as_str(), &aliased)
                .await?;
            let receipt = CommitChartReceipt {
                mutation: quire_server::report::ApplyOperationsReceipt {
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
            };
            return Ok(receipt);
        };

        let after_block_id = normalize_optional_text(request.after_block_id);
        let before_block_id = normalize_optional_text(request.before_block_id);
        let section_id = normalize_optional_text(request.section_id);
        if after_block_id.is_some() && before_block_id.is_some() {
            return Err(RuntimeError::InvalidRequest(
                "use at most one of `after_block_id` or `before_block_id`".into(),
            ));
        }
        if !preview.chart.option.is_object() {
            return Err(RuntimeError::InvalidRequest(
                "stored chart preview has no native ECharts `option`; call preview_echarts again"
                    .into(),
            ));
        }

        let target_id = BlockId::parse(request.block_id.trim())
            .map_err(|error| RuntimeError::InvalidRequest(error.to_string()))?;
        let raw_source_ids = request.source_ids.unwrap_or(preview.source_ids);
        let mut parsed_source_ids = Vec::with_capacity(raw_source_ids.len());
        for raw_id in raw_source_ids {
            let trimmed = raw_id.trim();
            if !trimmed.is_empty() {
                parsed_source_ids.push(
                    SourceId::parse(trimmed)
                        .map_err(|error| RuntimeError::InvalidRequest(error.to_string()))?,
                );
            }
        }
        let effective_source_note =
            normalize_optional_text(request.source_note).or(preview.source_note);

        let digest_receipt = self
            .report
            .get_document_digest(
                context.clone(),
                quire_server::report::DigestParams {
                    doc_id: doc_id.clone(),
                    query: None,
                    target_prose_chars: None,
                },
            )
            .await
            .map_err(report_error)?;
        let existing_echarts = digest_receipt
            .digest
            .blocks
            .iter()
            .any(|block| block.id == target_id.as_str() && block.block_type == "echarts");

        let expected_revision = match request.expected_revision {
            Some(value) if value >= 0 => value,
            Some(_) => {
                return Err(RuntimeError::InvalidRequest(
                    "`expected_revision` must be non-negative".into(),
                ));
            }
            None => digest_receipt.digest.current_rev,
        };

        let ops = if existing_echarts {
            let chart = preview.chart;
            let patch = BlockPatch {
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
                let anchor_id = BlockId::parse(anchor.trim())
                    .map_err(|error| RuntimeError::InvalidRequest(error.to_string()))?;
                if anchor_id != target_id {
                    patch_ops.push(PatchOp::MoveBlock {
                        block_id: target_id.clone(),
                        after_id: Some(anchor_id),
                    });
                }
            } else if let Some(anchor) = before_block_id {
                let anchor_id = BlockId::parse(anchor.trim())
                    .map_err(|error| RuntimeError::InvalidRequest(error.to_string()))?;
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
                            RuntimeError::InvalidRequest(format!(
                                "`before_block_id` anchor `{}` does not exist",
                                anchor_id.as_str()
                            ))
                        })?;
                    let already_before =
                        target_index.is_some_and(|index| index + 1 == anchor_index);
                    if !already_before {
                        let predecessor = anchor_index
                            .checked_sub(1)
                            .and_then(|index| block_ids.get(index))
                            .and_then(|id| (*id != target_id.as_str()).then_some(*id))
                            .map(|id| {
                                BlockId::parse(id).map_err(|error| {
                                    RuntimeError::InvalidRequest(error.to_string())
                                })
                            })
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
                    target_id: BlockId::parse(anchor.trim())
                        .map_err(|error| RuntimeError::InvalidRequest(error.to_string()))?,
                    blocks: vec![block],
                }
            } else if let Some(anchor) = before_block_id {
                PatchOp::InsertBefore {
                    target_id: BlockId::parse(anchor.trim())
                        .map_err(|error| RuntimeError::InvalidRequest(error.to_string()))?,
                    blocks: vec![block],
                }
            } else if let Some(section) = section_id {
                PatchOp::AppendToSection {
                    section_id: BlockId::parse(section.trim())
                        .map_err(|error| RuntimeError::InvalidRequest(error.to_string()))?,
                    blocks: vec![block],
                }
            } else {
                PatchOp::Append {
                    blocks: vec![block],
                }
            };
            vec![op]
        };

        let summary = request
            .summary
            .map(|value| value.trim().to_string())
            .filter(|value| !value.is_empty())
            .unwrap_or_else(|| format!("commit previewed chart `{target_id}`"));
        let receipt = self
            .report
            .exec_apply_operations(
                context,
                quire_server::report::ApplyOperationsParams {
                    doc_id,
                    expected_revision,
                    summary,
                    ops,
                    sources: None,
                    preview_ids: Vec::new(),
                },
            )
            .await
            .map_err(report_error)?;
        let receipt = CommitChartReceipt {
            mutation: receipt,
            preview_id,
            block_id: target_id.to_string(),
        };
        Ok(receipt)
    }
}

fn normalize_optional_text(value: Option<String>) -> Option<String> {
    value
        .map(|text| text.trim().to_string())
        .filter(|text| !text.is_empty())
}

fn store_error(error: quire_store::StoreError) -> RuntimeError {
    match error {
        quire_store::StoreError::Forbidden(message) => RuntimeError::Forbidden(message),
        other => RuntimeError::InvalidRequest(other.to_string()),
    }
}

fn report_error(error: rmcp::ErrorData) -> RuntimeError {
    RuntimeError::InvalidRequest(error.message.into_owned())
}

/// Map runtime failures to public rmcp errors without leaking internal storage details.
pub fn external_mcp_error(error: RuntimeError) -> rmcp::ErrorData {
    let internal = matches!(
        error,
        RuntimeError::Database(_)
            | RuntimeError::Redis(_)
            | RuntimeError::Json(_)
            | RuntimeError::Http(_)
            | RuntimeError::Io(_)
            | RuntimeError::Internal(_)
    );
    let code = error.code();
    let hint = error.hint();
    if internal {
        tracing::warn!(?error, "external preview tool failed internally");
        return rmcp::ErrorData::internal_error(
            "preview tool execution failed; inspect runtime logs or retry",
            Some(serde_json::json!({"code": code, "hint": hint})),
        );
    }
    rmcp::ErrorData::invalid_params(
        error.to_string(),
        Some(serde_json::json!({"code": code, "hint": hint})),
    )
}
