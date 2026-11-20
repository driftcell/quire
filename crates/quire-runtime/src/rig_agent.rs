//! rig-core tool adaptation, MCP HTTP transport and streamed preview throttling.

use std::{
    collections::{BTreeMap, BTreeSet, HashMap},
    sync::Arc,
    time::{Duration, Instant},
};

use async_trait::async_trait;
use base64::{Engine as _, engine::general_purpose::STANDARD as BASE64};
use futures::StreamExt;
use quire_lint::lint_echarts_block;
use quire_protocol::{
    Block, BlockId, BlockKind, EchartsBlock, EphemeralPayload, EphemeralPreviewEvent,
    PartialBlockPreview, PatchOp, PreviewAnchor, PreviewStatus, chart_data_tables_for_block,
    chart_requirements, merge_chart_option_patch, short_id,
};
use quire_session_protocol::{ContextUsage, MessageRole, QuestionItem};
use rig_core::{
    completion::{
        AssistantContent, CompletionRequest, CompletionResponse, Message, ToolDefinition, Usage,
        message::ToolResult as MessageToolResult,
    },
    message::{AssistantMessage, ImageMediaType, ToolCall, ToolName, UserContent},
    rig_tool,
    serve::adapters::ToolAdapter,
    streaming::{Item, PartKind, StreamEvent, parse_partial_arguments},
    tool::{
        DynamicTool, Tool, ToolContext, ToolExecutionError, ToolOutput,
        ToolResult as ExecutedToolResult,
    },
};
use secrecy::{ExposeSecret, SecretString};
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use tokio::{
    sync::{Mutex, mpsc},
    task::JoinSet,
};
use tokio_util::sync::CancellationToken;
use tracing::warn;

use crate::{
    comment_refs::render_stored_user_message,
    config::Config,
    error::{RuntimeError, RuntimeResult},
    kb::KbService,
    mcp_client::RmcpClient,
    mcp_registry::{McpRegistry, tool_pattern_matches},
    model_stream::{ModelEventStream, ModelStreamEvent, ModelStreamProvider, ModelTurnRequest},
    models::{ModelRegistry, ProviderProtocol},
    prompt::ContextMessage,
    sandbox::{SandboxExecRequest, SandboxLease, SandboxProvider, sandbox_spec_from_config},
    service::PendingQueueCoordinator,
    store::SessionPatch,
};

pub const BUILTIN_TOOLS: &[&str] = &[
    "load_skill",
    "read_skill_resource",
    "describe_task",
    "set_session_title",
    "list_subagents",
    "spawn_subagents",
    "ask_user",
    "preview_report",
    "preview_echarts",
    "commit_chart",
    "sandbox_execute",
    "sandbox_read_file",
    "sandbox_write_file",
];

/// Return configured patterns that match no assembled tool.
///
/// Wildcards are expanded against the final tool map rather than looked up as
/// literal names. A scoped pattern such as `server_a:subjectcloud_*` is checked
/// against the tool-name portion because assembled MCP tools keep bare names.
fn missing_allowed_tool_patterns(
    allowed_tools: &[String],
    tool_map: &BTreeMap<String, DynamicTool>,
) -> Vec<String> {
    let mut missing = Vec::new();
    for pattern in allowed_tools {
        let trimmed = pattern.trim();
        if trimmed.is_empty() {
            continue;
        }
        let effective = trimmed
            .split_once(':')
            .map(|(_, tool_name)| tool_name)
            .unwrap_or(trimmed)
            .trim();
        if effective.is_empty() || effective == "*" {
            continue;
        }
        let available = if effective.contains('*') {
            tool_map
                .keys()
                .any(|name| tool_pattern_matches(effective, name))
        } else {
            tool_map.contains_key(effective)
        };
        if !available {
            missing.push(trimmed.to_string());
        }
    }
    missing
}

fn warn_missing_allowed_tools(
    agent_id: &str,
    allowed_tools: &[String],
    tool_map: &BTreeMap<String, DynamicTool>,
) -> Vec<String> {
    let missing = missing_allowed_tool_patterns(allowed_tools, tool_map);
    for pattern in &missing {
        warn!(
            agent_id = %agent_id,
            allowed_tool = %pattern,
            "configured allowed_tool does not exist or is unavailable; skipping without blocking workflow"
        );
    }
    missing
}

fn allow_list_matches(allowed_tools: &[String], tool_name: &str) -> bool {
    allowed_tools
        .iter()
        .any(|pattern| tool_pattern_matches(pattern, tool_name))
}

/// Throttle for streamed tool-argument preview frames.
#[derive(Debug)]
pub struct PreviewThrottle {
    interval: Duration,
    last_emit: Option<Instant>,
}

impl PreviewThrottle {
    pub fn new(milliseconds: u64) -> Self {
        Self {
            interval: Duration::from_millis(milliseconds.clamp(50, 80)),
            last_emit: None,
        }
    }

    pub fn should_emit(&mut self) -> bool {
        self.should_emit_at(Instant::now())
    }

    fn should_emit_at(&mut self, now: Instant) -> bool {
        if self
            .last_emit
            .is_some_and(|last| now.duration_since(last) < self.interval)
        {
            return false;
        }
        self.last_emit = Some(now);
        true
    }
}

/// Sends partial tool-argument previews to Quire's ephemeral preview endpoint.
pub struct ToolPreviewStreamer {
    http: reqwest::Client,
    base_url: String,
    token: SecretString,
    doc_id: String,
    preview_id: String,
    actor: String,
    block_id: String,
    anchor: PreviewAnchor,
    seq: u64,
    throttle: PreviewThrottle,
}

impl ToolPreviewStreamer {
    pub fn new(
        base_url: impl Into<String>,
        token: SecretString,
        doc_id: impl Into<String>,
        preview_id: impl Into<String>,
        actor: impl Into<String>,
        block_id: impl Into<String>,
        throttle_ms: u64,
    ) -> Self {
        let block_id = block_id.into();
        Self {
            http: reqwest::Client::new(),
            base_url: base_url.into().trim_end_matches('/').to_string(),
            token,
            doc_id: doc_id.into(),
            preview_id: preview_id.into(),
            actor: actor.into(),
            anchor: PreviewAnchor::ReplaceBlock {
                block_id: quire_protocol::BlockId::pending(),
            },
            block_id,
            seq: 0,
            throttle: PreviewThrottle::new(throttle_ms),
        }
    }

    /// Override where the previewed block appears. Replace anchors are rewritten to the
    /// streamer's block ID.
    pub fn with_anchor(mut self, anchor: PreviewAnchor) -> Self {
        self.anchor = anchor;
        self
    }

    pub fn set_anchor(&mut self, anchor: PreviewAnchor) {
        self.anchor = anchor;
    }

    pub fn preview_id(&self) -> &str {
        &self.preview_id
    }

    pub async fn push_delta(&mut self, text: &str) -> RuntimeResult<bool> {
        let block_id = quire_protocol::BlockId::parse(self.block_id.clone())
            .map_err(|error| RuntimeError::InvalidRequest(error.to_string()))?;
        self.push_block(PartialBlockPreview {
            block_id: Some(block_id),
            block_type: "paragraph".into(),
            title: None,
            level: None,
            text: Some(text.to_string()),
        })
        .await
    }

    /// Push one partial block frame. The streamer remains responsible for throttling and
    /// monotonic sequence numbers.
    pub async fn push_block(&mut self, block: PartialBlockPreview) -> RuntimeResult<bool> {
        if !self.throttle.should_emit() {
            return Ok(false);
        }
        self.seq = self.seq.saturating_add(1);
        let block_id = quire_protocol::BlockId::parse(self.block_id.clone())
            .map_err(|error| RuntimeError::InvalidRequest(error.to_string()))?;
        let anchor = match self.anchor.clone() {
            PreviewAnchor::ReplaceBlock { .. } => PreviewAnchor::ReplaceBlock {
                block_id: block_id.clone(),
            },
            PreviewAnchor::AfterBlock { .. } => PreviewAnchor::AfterBlock {
                block_id: block_id.clone(),
            },
            PreviewAnchor::BeforeBlock { .. } => PreviewAnchor::BeforeBlock {
                block_id: block_id.clone(),
            },
            other => other,
        };
        let payload = EphemeralPayload::Preview {
            event: EphemeralPreviewEvent {
                doc_id: quire_protocol::DocId::parse(self.doc_id.clone())
                    .map_err(|error| RuntimeError::InvalidRequest(error.to_string()))?,
                preview_id: self.preview_id.clone(),
                actor: self.actor.clone(),
                anchor,
                status: PreviewStatus::Streaming,
                seq: Some(self.seq),
                ttl_ms: None,
                block,
                received_at: chrono::Utc::now(),
            },
        };
        let response = self
            .http
            .post(format!(
                "{}/api/docs/{}/ephemeral",
                self.base_url, self.doc_id
            ))
            .header(
                reqwest::header::AUTHORIZATION,
                format!("Bearer {}", self.token.expose_secret()),
            )
            .json(&payload)
            .send()
            .await?;
        if !response.status().is_success() {
            return Err(RuntimeError::Internal(format!(
                "preview publish failed with {}",
                response.status()
            )));
        }
        Ok(true)
    }
}

#[derive(Debug, Clone)]
struct PreviewImage {
    base64: String,
    mime_type: String,
    caption: String,
}

const REPORT_PREVIEW_CAPTION: &str = "Rendered screenshot from preview_report. Inspect visual layout, spacing, tables, and charts alongside the diagnostic JSON.";
const CHART_PREVIEW_CAPTION: &str = "Rendered ECharts screenshot from preview_echarts. Verify the chart type, series data, axis labels, legend, tooltip/static labels, colors, clipping/overflow, and that the visual matches fallback_summary. Refine with preview_echarts + based_on_preview_id + option_patch only, then call commit_chart once it looks right.";

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, rig_core::schemars::JsonSchema)]
pub struct SubagentTask {
    pub agent_name: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub display_name: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub model: Option<String>,
    pub task: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub section_id: Option<String>,
}

struct ToolBuildContext {
    execution: AgentExecution,
    scope: EventScope,
    engine: Arc<RigModelCore>,
    pending_preview_images: Arc<Mutex<Vec<PreviewImage>>>,
    supports_vision: bool,
    sandbox: Arc<SandboxLease>,
    doc_id: Arc<Mutex<Option<String>>>,
}

struct RuntimeToolScope {
    engine: Arc<RigModelCore>,
    execution: AgentExecution,
    event_scope: EventScope,
    pending_preview_images: Arc<Mutex<Vec<PreviewImage>>>,
    supports_vision: bool,
    sandbox: Arc<SandboxLease>,
    /// Latest bound report ID, shared across the turn so subagents spawned
    /// after a lazy `create_report` inherit the new document.
    doc_id: Arc<Mutex<Option<String>>>,
}

fn runtime_tool_scope(context: &ToolContext) -> Result<Arc<RuntimeToolScope>, ToolExecutionError> {
    context
        .scope::<RuntimeToolScope>()
        .ok_or_else(|| ToolExecutionError::other("missing runtime tool context"))
}

fn scoped_dynamic_tool<T>(
    scope: Arc<RuntimeToolScope>,
    factory: fn() -> T,
) -> RuntimeResult<DynamicTool>
where
    T: Tool + Default + 'static,
{
    let template = factory();
    let name = ToolName::new(T::NAME).map_err(|error| RuntimeError::Internal(error.to_string()))?;
    let description = template.description();
    let parameters = template.parameters();
    Ok(DynamicTool::new_with_context(
        name,
        description,
        parameters,
        move |context, arguments| {
            let mut context = context.clone().with_scope(scope.clone());
            let adapter = ToolAdapter::new(factory());
            Box::pin(async move { adapter.run_with(&mut context, arguments).await })
        },
    ))
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct SubagentOutcome {
    pub agent_name: String,
    pub status: String,
    pub summary: String,
}

fn tool_other(error: impl std::fmt::Display) -> ToolExecutionError {
    ToolExecutionError::other(error.to_string())
}

const MAX_TOOL_CODE_BYTES: usize = 256 * 1024;
const MAX_TOOL_FILE_BYTES: usize = 256 * 1024;
const MAX_TOOL_OUTPUT_BYTES: usize = 64 * 1024;

fn bounded_tool_text(text: String) -> (String, bool) {
    if text.len() <= MAX_TOOL_OUTPUT_BYTES {
        return (text, false);
    }
    let mut end = MAX_TOOL_OUTPUT_BYTES;
    while !text.is_char_boundary(end) {
        end = end.saturating_sub(1);
    }
    (text.get(..end).unwrap_or_default().to_string(), true)
}

#[rig_tool(
    name = "sandbox_execute",
    description = "Execute Python, JavaScript or Bash in this run's isolated remote sandbox. Files and installed dependencies persist across sandbox calls in the same run. The sandbox is created on first use and destroyed when the run ends."
)]
async fn sandbox_execute_tool(
    #[rig(context)] context: &mut ToolContext,
    language: String,
    code: String,
    cwd: Option<String>,
    timeout_seconds: Option<u64>,
) -> Result<Value, ToolExecutionError> {
    if code.len() > MAX_TOOL_CODE_BYTES {
        return Err(ToolExecutionError::invalid_args(format!(
            "code exceeds the {MAX_TOOL_CODE_BYTES}-byte tool limit"
        )));
    }
    let runtime = runtime_tool_scope(context)?;
    let sandbox = runtime.sandbox.get_or_create().await.map_err(tool_other)?;
    let result = sandbox
        .exec_code(SandboxExecRequest {
            language,
            code,
            cwd,
            timeout_seconds,
        })
        .await
        .map_err(tool_other)?;
    let (stdout, stdout_bounded) = bounded_tool_text(result.stdout);
    let (stderr, stderr_bounded) = bounded_tool_text(result.stderr);
    Ok(json!({
        "sandbox_id": sandbox.id(),
        "exit_code": result.exit_code,
        "stdout": stdout,
        "stderr": stderr,
        "stdout_truncated": result.stdout_truncated || stdout_bounded,
        "stderr_truncated": result.stderr_truncated || stderr_bounded,
    }))
}

#[rig_tool(
    name = "sandbox_read_file",
    description = "Read a file from this run's sandbox. UTF-8 text is returned directly; binary content is base64 encoded. Files larger than 256 KiB must be chunked with sandbox_execute."
)]
async fn sandbox_read_file_tool(
    #[rig(context)] context: &mut ToolContext,
    path: String,
) -> Result<Value, ToolExecutionError> {
    if path.trim().is_empty() {
        return Err(ToolExecutionError::invalid_args(
            "path cannot be empty".to_string(),
        ));
    }
    let runtime = runtime_tool_scope(context)?;
    let sandbox = runtime.sandbox.get_or_create().await.map_err(tool_other)?;
    let bytes = sandbox.download_file(&path).await.map_err(tool_other)?;
    if bytes.len() > MAX_TOOL_FILE_BYTES {
        return Err(ToolExecutionError::other(format!(
            "file exceeds the {MAX_TOOL_FILE_BYTES}-byte tool limit; read a chunk with sandbox_execute"
        )));
    }
    let (encoding, content) = match String::from_utf8(bytes) {
        Ok(text) => ("utf-8".to_string(), text),
        Err(error) => ("base64".to_string(), BASE64.encode(error.into_bytes())),
    };
    Ok(json!({
        "sandbox_id": sandbox.id(),
        "path": path,
        "encoding": encoding,
        "content": content,
    }))
}

#[rig_tool(
    name = "sandbox_write_file",
    description = "Write a UTF-8 text file in this run's sandbox, creating missing parent directories when the provider supports it. The content limit is 256 KiB."
)]
async fn sandbox_write_file_tool(
    #[rig(context)] context: &mut ToolContext,
    path: String,
    content: String,
) -> Result<Value, ToolExecutionError> {
    if path.trim().is_empty() {
        return Err(ToolExecutionError::invalid_args(
            "path cannot be empty".to_string(),
        ));
    }
    if content.len() > MAX_TOOL_FILE_BYTES {
        return Err(ToolExecutionError::invalid_args(format!(
            "content exceeds the {MAX_TOOL_FILE_BYTES}-byte tool limit"
        )));
    }
    let runtime = runtime_tool_scope(context)?;
    let sandbox = runtime.sandbox.get_or_create().await.map_err(tool_other)?;
    sandbox
        .upload_file(&path, content.as_bytes())
        .await
        .map_err(tool_other)?;
    Ok(json!({
        "sandbox_id": sandbox.id(),
        "path": path,
        "bytes_written": content.len(),
    }))
}

#[allow(clippy::too_many_arguments)]
#[rig_tool(
    name = "preview_report",
    description = "Render a preview of the Quire report via controlled Playwright MCP, returning layout diagnostics and a public preview URL. Vision-capable models also receive the screenshot."
)]
async fn preview_report_tool(
    #[rig(context)] context: &mut ToolContext,
    doc_id: String,
    mode: Option<crate::preview::PreviewViewMode>,
    section_id: Option<String>,
    block_ids: Option<Vec<String>>,
    page: Option<usize>,
    rev: Option<u64>,
    include_styles: Option<bool>,
) -> Result<Value, ToolExecutionError> {
    let runtime = runtime_tool_scope(context)?;
    let input = crate::preview::PreviewReportInput {
        doc_id,
        mode: mode.unwrap_or_default(),
        section_id,
        block_ids,
        page,
        rev,
        include_styles: include_styles.unwrap_or(true),
        include_image: runtime.supports_vision,
    };
    let result = crate::preview::execute_preview_report(&runtime.engine.config, input)
        .await
        .map_err(tool_other)?;
    if let (Some(base64), Some(mime_type)) =
        (result.image_base64.clone(), result.image_mime_type.clone())
    {
        runtime
            .pending_preview_images
            .lock()
            .await
            .push(PreviewImage {
                base64,
                mime_type,
                caption: REPORT_PREVIEW_CAPTION.to_string(),
            });
    }
    Ok(result.to_diagnostics_json())
}

#[allow(clippy::too_many_arguments)]
#[rig_tool(
    name = "preview_echarts",
    description = "Validate and render one ECharts chart before writing it. First call: send the complete native ECharts 6.0 `option`. Refinement calls: send `based_on_preview_id` plus a small `option_patch`; never resend the full option. Returns preview_id, lint_issues and preview_url. Vision models also receive a screenshot. Commit with commit_chart."
)]
async fn preview_echarts_tool(
    #[rig(context)] context: &mut ToolContext,
    option: Option<Value>,
    option_patch: Option<Value>,
    based_on_preview_id: Option<String>,
    height: Option<u32>,
    title: Option<String>,
    note: Option<String>,
    fallback_summary: Option<String>,
) -> Result<Value, ToolExecutionError> {
    let runtime = runtime_tool_scope(context)?;
    let config = runtime.engine.config.as_ref();
    let bus = runtime.engine.kb.store().bus();
    let based_on = based_on_preview_id
        .as_deref()
        .map(str::trim)
        .filter(|value| !value.is_empty());

    let mut based_on_id = None;
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
    let lint = lint_echarts_block(preview_block_id, &chart);
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
    };
    bus.store_chart_preview(&record).await.map_err(tool_other)?;

    let mut preview_url = crate::preview::build_public_chart_preview_url(config, &preview_id);
    let mut preview_error = None;
    let mut image_included = false;
    if runtime.supports_vision {
        match crate::preview::execute_preview_echarts(config, &preview_id).await {
            Ok(result) => {
                preview_url = result.preview_url;
                if let (Some(base64), Some(mime_type)) =
                    (result.image_base64, result.image_mime_type)
                {
                    runtime
                        .pending_preview_images
                        .lock()
                        .await
                        .push(PreviewImage {
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

    let lint_issues = lint.diagnostics;
    let lint_summary = lint.summary;
    Ok(json!({
        "preview_id": preview_id,
        "preview_url": preview_url,
        "lint_issues": lint_issues,
        "lint_summary": lint_summary,
        "image_included": image_included,
        "preview_error": preview_error,
    }))
}

fn normalize_optional_text(value: Option<String>) -> Option<String> {
    value
        .map(|text| text.trim().to_string())
        .filter(|text| !text.is_empty())
}

#[allow(clippy::too_many_arguments)]
#[rig_tool(
    name = "commit_chart",
    description = "Commit a chart that already passed preview_echarts into the report. Accepts only preview_id, doc_id, block_id and an insertion anchor; never send the ECharts option again. Omit expected_revision to read the current revision first. Use after visual and lint review passes."
)]
async fn commit_chart_tool(
    #[rig(context)] context: &mut ToolContext,
    preview_id: String,
    doc_id: String,
    block_id: String,
    after_block_id: Option<String>,
    before_block_id: Option<String>,
    section_id: Option<String>,
    expected_revision: Option<i64>,
    summary: Option<String>,
) -> Result<Value, ToolExecutionError> {
    let runtime = runtime_tool_scope(context)?;
    let preview_id = preview_id.trim().to_string();
    if preview_id.is_empty() {
        return Err(ToolExecutionError::invalid_args(
            "`preview_id` must not be empty",
        ));
    }
    let doc_id = doc_id.trim().to_string();
    if doc_id.is_empty() {
        return Err(ToolExecutionError::invalid_args(
            "`doc_id` must not be empty",
        ));
    }
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
    let anchors = [
        after_block_id.is_some(),
        before_block_id.is_some(),
        section_id.is_some(),
    ];
    if anchors.iter().filter(|present| **present).count() > 1 {
        return Err(ToolExecutionError::invalid_args(
            "use at most one of `after_block_id`, `before_block_id` or `section_id`",
        ));
    }
    if !preview.chart.option.is_object() {
        return Err(ToolExecutionError::invalid_args(
            "stored chart preview has no native ECharts `option`; call preview_echarts again",
        ));
    }

    let target_id = BlockId::parse(block_id.trim()).map_err(tool_other)?;
    let block = Block::new(target_id.clone(), BlockKind::Echarts(preview.chart));
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

    let config = runtime.engine.config.as_ref();
    let client = RmcpClient::connect(
        &config.quire_base_url,
        Some(config.quire_rw_token.clone()),
        &runtime.execution.session_id,
        &runtime.execution.agent_id,
    )
    .await
    .map_err(tool_other)?;
    let expected_revision = match expected_revision {
        Some(value) if value >= 0 => value,
        Some(_) => {
            return Err(ToolExecutionError::invalid_args(
                "`expected_revision` must be non-negative",
            ));
        }
        None => {
            let digest = client
                .call_tool("get_document_digest", json!({ "doc_id": doc_id }))
                .await
                .map_err(tool_other)?;
            digest
                .get("digest")
                .and_then(|digest| digest.get("current_rev"))
                .and_then(Value::as_i64)
                .ok_or_else(|| {
                    ToolExecutionError::other(
                        "get_document_digest did not return digest.current_rev",
                    )
                })?
        }
    };
    let summary = summary
        .map(|value| value.trim().to_string())
        .filter(|value| !value.is_empty())
        .unwrap_or_else(|| format!("commit previewed chart `{target_id}`"));
    let receipt = client
        .call_tool(
            "apply_operations",
            json!({
                "doc_id": doc_id,
                "expected_revision": expected_revision,
                "summary": summary,
                "ops": [serde_json::to_value(&op).map_err(tool_other)?],
            }),
        )
        .await
        .map_err(tool_other)?;
    let mut payload = receipt;
    if let Some(object) = payload.as_object_mut() {
        object.insert("preview_id".to_string(), json!(preview_id));
        object.insert("block_id".to_string(), json!(target_id.as_str()));
    }
    Ok(payload)
}

#[rig_tool(
    name = "load_skill",
    description = "Load a skill definition from the session knowledge-base branch."
)]
async fn load_skill_tool(
    #[rig(context)] context: &mut ToolContext,
    name: String,
) -> Result<Value, ToolExecutionError> {
    let runtime = runtime_tool_scope(context)?;
    let skill = runtime
        .engine
        .kb
        .get_skill_on(runtime.execution.kb_branch.as_deref(), &name)
        .await
        .map_err(tool_other)?
        .ok_or_else(|| ToolExecutionError::not_found(format!("skill `{name}`")))?;
    serde_json::to_value(skill).map_err(tool_other)
}

#[rig_tool(
    name = "read_skill_resource",
    description = "Read one resource file bundled with a skill on the session branch."
)]
async fn read_skill_resource_tool(
    #[rig(context)] context: &mut ToolContext,
    skill: String,
    path: String,
) -> Result<Value, ToolExecutionError> {
    let runtime = runtime_tool_scope(context)?;
    let definition = runtime
        .engine
        .kb
        .get_skill_on(runtime.execution.kb_branch.as_deref(), &skill)
        .await
        .map_err(tool_other)?
        .ok_or_else(|| ToolExecutionError::not_found(format!("skill `{skill}`")))?;
    let resource = definition
        .resources
        .iter()
        .find(|resource| resource.path == path)
        .ok_or_else(|| ToolExecutionError::not_found(format!("resource `{path}` in `{skill}`")))?;
    Ok(json!({
        "skill": skill,
        "path": path,
        "content": resource.content,
    }))
}

#[rig_tool(
    name = "describe_task",
    description = "Publish the current task focus to the frontend event stream."
)]
fn describe_task_tool(
    #[rig(context)] context: &mut ToolContext,
    title: String,
    detail: Option<String>,
    section_id: Option<String>,
) -> Result<Value, ToolExecutionError> {
    let runtime = runtime_tool_scope(context)?;
    runtime.event_scope.send(ModelStreamEvent::TaskFocus {
        title: title.clone(),
        detail: detail.clone(),
        section_id: section_id.clone(),
    });
    Ok(json!({
        "title": title,
        "detail": detail,
        "section_id": section_id,
    }))
}

#[rig_tool(
    name = "set_session_title",
    description = "Set or refine the current chat session title once the user's topic is clear."
)]
async fn set_session_title_tool(
    #[rig(context)] context: &mut ToolContext,
    title: String,
) -> Result<Value, ToolExecutionError> {
    let runtime = runtime_tool_scope(context)?;
    let title: String = title.trim().chars().take(60).collect();
    if title.is_empty() {
        return Err(ToolExecutionError::invalid_args(
            "`title` must be non-empty",
        ));
    }
    runtime
        .engine
        .kb
        .store()
        .update_session(
            &runtime.execution.session_id,
            SessionPatch {
                title: Some(&title),
                ..SessionPatch::default()
            },
        )
        .await
        .map_err(tool_other)?;
    runtime
        .event_scope
        .send(ModelStreamEvent::SessionTitleUpdated {
            title: title.clone(),
        });
    Ok(json!({
        "session_id": runtime.execution.session_id,
        "title": title,
    }))
}

#[rig_tool(
    name = "ask_user",
    description = "Pause the run and request structured choices from the user."
)]
fn ask_user_tool(
    #[rig(context)] _context: &mut ToolContext,
    questions: Vec<QuestionItem>,
) -> Result<Value, ToolExecutionError> {
    let _ = questions;
    Err(ToolExecutionError::other(
        "ask_user is handled by the runtime question loop",
    ))
}

#[rig_tool(
    name = "list_subagents",
    description = "List available subagent names and descriptions on this branch."
)]
async fn list_subagents_tool(
    #[rig(context)] context: &mut ToolContext,
) -> Result<Value, ToolExecutionError> {
    let runtime = runtime_tool_scope(context)?;
    let agents = runtime
        .engine
        .kb
        .list_agents_on(runtime.execution.kb_branch.as_deref())
        .await
        .map_err(tool_other)?;
    let subagents: Vec<Value> = agents
        .into_iter()
        .filter(|agent| {
            agent.enabled && !agent.is_default && agent.name != runtime.execution.agent_id
        })
        .map(|agent| {
            json!({
                "name": agent.name,
                "description": agent.description,
                "model": agent.model,
                "allowed_tools": agent.allowed_tools,
            })
        })
        .collect();
    Ok(json!({ "subagents": subagents }))
}

#[rig_tool(
    name = "spawn_subagents",
    description = "Run multiple subagents concurrently with a JoinSet. Call list_subagents first to get valid subagent names."
)]
async fn run_subagents_tool(
    #[rig(context)] context: &mut ToolContext,
    tasks: Vec<SubagentTask>,
) -> Result<Value, ToolExecutionError> {
    let runtime = runtime_tool_scope(context)?;
    let bound_doc_id = runtime.doc_id.lock().await.clone();
    let mut joinset = JoinSet::new();
    for task in tasks {
        let engine = Arc::clone(&runtime.engine);
        let scope = runtime.event_scope.clone();
        let mut execution = runtime.execution.clone();
        execution.doc_id = bound_doc_id.clone();
        joinset.spawn(async move { engine.run_subagent(scope, execution, task).await });
    }
    let mut outcomes = Vec::new();
    while let Some(joined) = joinset.join_next().await {
        match joined {
            Ok(outcome) => outcomes.push(outcome),
            Err(error) => outcomes.push(SubagentOutcome {
                agent_name: "unknown".into(),
                status: "failed".into(),
                summary: format!("subagent join error: {error}"),
            }),
        }
    }
    serde_json::to_value(outcomes).map_err(tool_other)
}

/// Run subagent closures concurrently with a bounded JoinSet.
pub async fn spawn_subagents<F, Fut>(
    tasks: Vec<SubagentTask>,
    runner: F,
) -> RuntimeResult<Vec<SubagentOutcome>>
where
    F: Fn(SubagentTask) -> Fut + Send + Sync + 'static,
    Fut: std::future::Future<Output = SubagentOutcome> + Send + 'static,
{
    let mut joinset = tokio::task::JoinSet::new();
    for task in tasks {
        joinset.spawn(runner(task));
    }
    let mut outcomes = Vec::new();
    while let Some(result) = joinset.join_next().await {
        outcomes
            .push(result.map_err(|error| {
                RuntimeError::Internal(format!("subagent join error: {error}"))
            })?);
    }
    Ok(outcomes)
}

const PREVIEW_TOOLS: &[&str] = &["apply_operations"];

#[derive(Clone)]
pub struct RigModelStreamProvider {
    core: Arc<RigModelCore>,
}

impl RigModelStreamProvider {
    pub fn new(
        kb: KbService,
        models: Arc<ModelRegistry>,
        config: Arc<Config>,
        mcp: Arc<McpRegistry>,
        sandbox_provider: Arc<dyn SandboxProvider>,
        queue: PendingQueueCoordinator,
    ) -> Self {
        Self {
            core: Arc::new(RigModelCore {
                kb,
                models,
                config,
                mcp,
                sandbox_provider,
                queue,
            }),
        }
    }
}

#[async_trait]
impl ModelStreamProvider for RigModelStreamProvider {
    async fn stream_turn(&self, request: ModelTurnRequest) -> RuntimeResult<ModelEventStream> {
        let core = self.core.clone();
        let cancellation = request.cancellation.clone();
        let (sender, mut receiver) = mpsc::unbounded_channel::<RuntimeResult<ModelStreamEvent>>();
        tokio::spawn(async move {
            if let Err(error) = core.run_main_turn(request, sender.clone()).await {
                let _ = sender.send(Err(error));
            }
        });
        let stream = async_stream::stream! {
            loop {
                tokio::select! {
                    _ = cancellation.cancelled() => break,
                    item = receiver.recv() => {
                        match item {
                            Some(item) => yield item,
                            None => break,
                        }
                    }
                }
            }
        };
        Ok(Box::pin(stream))
    }
}

struct RigModelCore {
    kb: KbService,
    models: Arc<ModelRegistry>,
    config: Arc<Config>,
    mcp: Arc<McpRegistry>,
    sandbox_provider: Arc<dyn SandboxProvider>,
    queue: PendingQueueCoordinator,
}

#[derive(Clone)]
struct AgentExecution {
    agent_id: String,
    model_route: String,
    allowed_tools: Vec<String>,
    disable_tools: bool,
    system_prompt: String,
    kb_branch: Option<String>,
    doc_id: Option<String>,
    session_id: String,
    run_id: String,
    cancellation: CancellationToken,
}

#[derive(Clone)]
struct SubagentScope {
    subagent_run_id: String,
    agent_name: String,
}

#[derive(Clone)]
struct EventScope {
    sender: mpsc::UnboundedSender<RuntimeResult<ModelStreamEvent>>,
    subagent: Option<SubagentScope>,
    cancellation: CancellationToken,
}

impl EventScope {
    fn top(
        sender: mpsc::UnboundedSender<RuntimeResult<ModelStreamEvent>>,
        token: CancellationToken,
    ) -> Self {
        Self {
            sender,
            subagent: None,
            cancellation: token,
        }
    }

    fn child(&self, subagent_run_id: String, agent_name: String) -> Self {
        Self {
            sender: self.sender.clone(),
            subagent: Some(SubagentScope {
                subagent_run_id,
                agent_name,
            }),
            cancellation: self.cancellation.clone(),
        }
    }

    fn send(&self, event: ModelStreamEvent) {
        let event = match &self.subagent {
            Some(scope) => ModelStreamEvent::SubagentEvent {
                subagent_run_id: scope.subagent_run_id.clone(),
                agent_name: scope.agent_name.clone(),
                event: Box::new(event),
            },
            None => event,
        };
        let _ = self.sender.send(Ok(event));
    }

    fn send_raw(&self, event: ModelStreamEvent) {
        let _ = self.sender.send(Ok(event));
    }
}

impl RigModelCore {
    async fn run_main_turn(
        self: Arc<Self>,
        request: ModelTurnRequest,
        sender: mpsc::UnboundedSender<RuntimeResult<ModelStreamEvent>>,
    ) -> RuntimeResult<()> {
        let branch = request.kb_branch.clone();
        let agent = self
            .kb
            .get_agent_on(branch.as_deref(), &request.agent_id)
            .await?
            .ok_or_else(|| {
                RuntimeError::InvalidRequest(format!(
                    "agent `{}` is not defined on branch `{}`; configure it with kb_upsert_agent",
                    request.agent_id,
                    branch.as_deref().unwrap_or("active")
                ))
            })?;
        let model_route = if request.model_route.trim().is_empty() {
            agent.model.clone()
        } else {
            request.model_route.clone()
        };
        let allowed_tools = if request.disable_tools {
            Vec::new()
        } else if request.allowed_tools.is_empty() {
            agent.allowed_tools.clone()
        } else {
            request.allowed_tools.clone()
        };
        let messages = messages_from_request(&request);
        let execution = AgentExecution {
            agent_id: request.agent_id,
            model_route,
            allowed_tools,
            disable_tools: request.disable_tools,
            system_prompt: request.system_prompt,
            kb_branch: branch,
            doc_id: request.doc_id,
            session_id: request.session_id,
            run_id: request.run_id,
            cancellation: request.cancellation.clone(),
        };
        let scope = EventScope::top(sender, request.cancellation);
        self.run_turn(execution, messages, scope).await?;
        Ok(())
    }

    async fn run_turn(
        self: Arc<Self>,
        mut execution: AgentExecution,
        mut messages: Vec<Message>,
        scope: EventScope,
    ) -> RuntimeResult<String> {
        let resolved = self
            .models
            .resolve(&execution.model_route)
            .await
            .map_err(|error| {
                RuntimeError::InvalidRequest(format!(
                    "model route `{}` is not configured: {error}",
                    execution.model_route
                ))
            })?;
        let context_limit = u64::try_from(resolved.context_limit).unwrap_or(0);
        let max_tokens = match resolved
            .max_output_tokens
            .and_then(|value| u64::try_from(value).ok())
        {
            Some(value) => Some(value),
            None if resolved.protocol == ProviderProtocol::Anthropic => Some(8192),
            None => None,
        };
        let model = self
            .models
            .build_rig_model(&execution.model_route)
            .await
            .map_err(|error| {
                RuntimeError::InvalidRequest(format!(
                    "model route `{}` cannot be built: {error}",
                    execution.model_route
                ))
            })?;
        let supports_vision = resolved.supports_vision;
        let pending_preview_images: Arc<Mutex<Vec<PreviewImage>>> =
            Arc::new(Mutex::new(Vec::new()));
        let doc_id_state = Arc::new(Mutex::new(execution.doc_id.clone()));
        let mut sandbox_spec = sandbox_spec_from_config(&self.config.sandbox);
        sandbox_spec
            .metadata
            .insert("quire_run_id".into(), execution.run_id.clone());
        sandbox_spec
            .metadata
            .insert("quire_session_id".into(), execution.session_id.clone());
        let sandbox_lease = Arc::new(SandboxLease::new(
            Arc::clone(&self.sandbox_provider),
            sandbox_spec,
        ));
        let mut tools = self.build_tools(ToolBuildContext {
            execution: execution.clone(),
            scope: scope.clone(),
            engine: Arc::clone(&self),
            pending_preview_images: Arc::clone(&pending_preview_images),
            supports_vision,
            sandbox: Arc::clone(&sandbox_lease),
            doc_id: Arc::clone(&doc_id_state),
        })?;
        for binding in self.mcp.tools_for_agent(&execution.allowed_tools).await? {
            let allowed: Vec<String> = binding
                .tool_names
                .iter()
                .filter(|name| !BUILTIN_TOOLS.contains(&name.as_str()))
                .cloned()
                .collect();
            if allowed.is_empty() {
                continue;
            }
            match RmcpClient::connect(
                &binding.url,
                binding.auth_token,
                &execution.session_id,
                &execution.agent_id,
            )
            .await
            {
                Ok(client) => match client.dynamic_tools(&allowed) {
                    Ok(mut remote) => tools.append(&mut remote),
                    Err(error) => {
                        warn!(
                            %error,
                            server_id = %binding.server_id,
                            server_name = %binding.name,
                            "skipping unavailable MCP server while assembling tools"
                        );
                    }
                },
                Err(error) => {
                    warn!(
                        %error,
                        server_id = %binding.server_id,
                        server_name = %binding.name,
                        "skipping unavailable MCP server while assembling tools"
                    );
                }
            }
        }
        let tool_map: BTreeMap<String, DynamicTool> = tools
            .into_iter()
            .map(|tool| (tool.name().as_str().to_string(), tool))
            .collect();
        warn_missing_allowed_tools(&execution.agent_id, &execution.allowed_tools, &tool_map);
        let mut last_usage: Usage;
        let mut final_text = String::new();
        let mut suspended = false;
        loop {
            if execution.cancellation.is_cancelled() {
                return Ok(final_text);
            }
            let definitions: Vec<ToolDefinition> =
                tool_map.values().map(|tool| tool.definition()).collect();
            let request = build_completion_request(&execution, &messages, definitions, max_tokens);
            if let Err(error) = model.check(&request) {
                warn!(
                    %error,
                    model = %execution.model_route,
                    "model request preflight reported unsupported options"
                );
            }
            let mut stream = model.stream(request).map_err(model_provider_error)?;
            let mut previews = PreviewState::new(&execution, &self.config);
            let mut streamed_text = false;
            loop {
                tokio::select! {
                    _ = execution.cancellation.cancelled() => {
                        return Ok(final_text);
                    }
                    item = stream.next() => {
                        match item {
                            Some(Ok(Item::Event(event))) => {
                                if handle_stream_event(
                                    &event,
                                    &scope,
                                    &mut previews,
                                )
                                .await
                                {
                                    streamed_text = true;
                                }
                            }
                            Some(Ok(Item::Unknown(_))) => {}
                            Some(Err(error)) => return Err(model_provider_error(error)),
                            None => break,
                        }
                    }
                }
            }
            let response = stream.finish().await.map_err(model_provider_error)?;
            // Context occupancy is the final request's prompt size, not the
            // sum of every tool-round prompt. Keep the last call's usage so the
            // session meter reflects the live context window.
            last_usage = response.usage;
            let text = response_text(&response);
            if !streamed_text && !text.is_empty() {
                scope.send(ModelStreamEvent::TextDelta(text.clone()));
            }
            let calls: Vec<ToolCall> = response
                .choice
                .iter()
                .filter_map(|content| match content {
                    AssistantContent::ToolCall(call) => Some(call.clone()),
                    _ => None,
                })
                .collect();
            if execution.disable_tools {
                final_text = text;
                break;
            }
            if calls.is_empty() {
                final_text = text;
                break;
            }
            messages.push(Message::Assistant(AssistantMessage::new(
                response.choice.clone(),
            )));
            let mut answers: Vec<MessageToolResult> = Vec::new();
            for call in calls {
                if !execution.run_id.starts_with("run_sub_")
                    && call.function.name.as_str() == "ask_user"
                {
                    request_questionnaire(&scope, &execution, call).await?;
                    suspended = true;
                    break;
                }
                let (answer, linked_doc) =
                    execute_tool_call(&tool_map, &scope, &execution, &mut previews, call).await;
                if let Some((doc_id, title)) = linked_doc
                    && !execution.run_id.starts_with("run_sub_")
                {
                    if let Err(error) = self
                        .kb
                        .store()
                        .bind_doc(&execution.session_id, &doc_id)
                        .await
                    {
                        tracing::warn!(%error, %doc_id, session_id = %execution.session_id, "failed to bind lazily created report");
                    } else {
                        execution.doc_id = Some(doc_id.clone());
                        *doc_id_state.lock().await = Some(doc_id.clone());
                        scope.send(ModelStreamEvent::DocLinked { doc_id, title });
                    }
                }
                answers.push(answer);
            }
            if suspended {
                break;
            }
            messages.push(Message::tool_results(answers));
            let preview_images = {
                let mut guard = pending_preview_images.lock().await;
                std::mem::take(&mut *guard)
            };
            if !preview_images.is_empty() {
                let mut parts = Vec::new();
                for image in preview_images {
                    if !image.caption.trim().is_empty() {
                        parts.push(UserContent::text(image.caption));
                    }
                    let media_type = match image.mime_type.as_str() {
                        "image/jpeg" => ImageMediaType::JPEG,
                        "image/webp" => ImageMediaType::WEBP,
                        _ => ImageMediaType::PNG,
                    };
                    parts.push(UserContent::image_base64(
                        image.base64,
                        Some(media_type),
                        None,
                    ));
                }
                if !parts.is_empty() {
                    messages.push(Message::User { content: parts });
                }
            }
            if !execution.run_id.starts_with("run_sub_")
                && let Some(batch) = self
                    .queue
                    .take_pending_batch(&execution.session_id, &execution.run_id)
                    .await?
            {
                // Route the event through the ordered stream so it is persisted after the tool
                // result that preceded it, never before.
                scope.send(ModelStreamEvent::UserMessageInjected {
                    message_id: batch.message.message_id.clone(),
                    content: batch.message.content.clone(),
                    created_at: batch.message.created_at,
                });
                messages.push(Message::user(render_stored_user_message(&batch.message)));
            }
        }
        if let Err(error) = sandbox_lease.close().await {
            warn!(%error, run_id = %execution.run_id, "sandbox cleanup failed; provider TTL remains the backstop");
        }
        if execution.cancellation.is_cancelled() {
            return Ok(final_text);
        }
        scope.send(ModelStreamEvent::Usage(context_usage(
            &last_usage,
            context_limit,
        )));
        Ok(final_text)
    }

    fn build_tools(&self, context: ToolBuildContext) -> RuntimeResult<Vec<DynamicTool>> {
        let ToolBuildContext {
            execution,
            scope,
            engine,
            pending_preview_images,
            supports_vision,
            sandbox,
            doc_id,
        } = context;
        let runtime = Arc::new(RuntimeToolScope {
            engine,
            execution: execution.clone(),
            event_scope: scope,
            pending_preview_images,
            supports_vision,
            sandbox,
            doc_id,
        });
        let mut tools = Vec::new();
        if allow_list_matches(&execution.allowed_tools, "preview_report") {
            tools.push(scoped_dynamic_tool(Arc::clone(&runtime), || {
                PreviewReportTool
            })?);
        }
        if allow_list_matches(&execution.allowed_tools, "preview_echarts") {
            tools.push(scoped_dynamic_tool(Arc::clone(&runtime), || {
                PreviewEchartsTool
            })?);
        }
        if allow_list_matches(&execution.allowed_tools, "commit_chart") {
            tools.push(scoped_dynamic_tool(Arc::clone(&runtime), || {
                CommitChartTool
            })?);
        }
        if allow_list_matches(&execution.allowed_tools, "sandbox_execute") {
            tools.push(scoped_dynamic_tool(Arc::clone(&runtime), || {
                SandboxExecuteTool
            })?);
        }
        if allow_list_matches(&execution.allowed_tools, "sandbox_read_file") {
            tools.push(scoped_dynamic_tool(Arc::clone(&runtime), || {
                SandboxReadFileTool
            })?);
        }
        if allow_list_matches(&execution.allowed_tools, "sandbox_write_file") {
            tools.push(scoped_dynamic_tool(Arc::clone(&runtime), || {
                SandboxWriteFileTool
            })?);
        }
        if allow_list_matches(&execution.allowed_tools, "load_skill") {
            tools.push(scoped_dynamic_tool(Arc::clone(&runtime), || LoadSkillTool)?);
        }
        if allow_list_matches(&execution.allowed_tools, "read_skill_resource") {
            tools.push(scoped_dynamic_tool(Arc::clone(&runtime), || {
                ReadSkillResourceTool
            })?);
        }
        if allow_list_matches(&execution.allowed_tools, "describe_task") {
            tools.push(scoped_dynamic_tool(Arc::clone(&runtime), || {
                DescribeTaskTool
            })?);
        }
        if !execution.run_id.starts_with("run_sub_")
            && (allow_list_matches(&execution.allowed_tools, "set_session_title")
                || allow_list_matches(&execution.allowed_tools, "describe_task"))
        {
            tools.push(scoped_dynamic_tool(Arc::clone(&runtime), || {
                SetSessionTitleTool
            })?);
        }
        if !execution.run_id.starts_with("run_sub_")
            && allow_list_matches(&execution.allowed_tools, "ask_user")
        {
            tools.push(scoped_dynamic_tool(Arc::clone(&runtime), || AskUserTool)?);
        }
        if allow_list_matches(&execution.allowed_tools, "list_subagents")
            || (!execution.run_id.starts_with("run_sub_")
                && allow_list_matches(&execution.allowed_tools, "spawn_subagents"))
        {
            tools.push(scoped_dynamic_tool(Arc::clone(&runtime), || {
                ListSubagentsTool
            })?);
        }
        if allow_list_matches(&execution.allowed_tools, "spawn_subagents") {
            tools.push(scoped_dynamic_tool(Arc::clone(&runtime), || {
                RunSubagentsTool
            })?);
        }
        Ok(tools)
    }

    async fn run_subagent(
        self: Arc<Self>,
        parent_scope: EventScope,
        parent: AgentExecution,
        task: SubagentTask,
    ) -> SubagentOutcome {
        let subagent_run_id = short_id("run_sub");
        parent_scope.send_raw(ModelStreamEvent::SubagentStarted {
            subagent_run_id: subagent_run_id.clone(),
            agent_name: task.agent_name.clone(),
            display_name: task.display_name.clone(),
            task: task.task.clone(),
            section_id: task.section_id.clone(),
        });
        let result = async {
            if parent.cancellation.is_cancelled() {
                return Err(RuntimeError::InvalidRequest("run cancelled".into()));
            }
            let definition = self
                .kb
                .get_agent_on(parent.kb_branch.as_deref(), &task.agent_name)
                .await?
                .ok_or_else(|| {
                    RuntimeError::InvalidRequest(format!(
                        "subagent `{}` is not defined on branch `{}`; call list_subagents to inspect available subagents",
                        task.agent_name,
                        parent.kb_branch.as_deref().unwrap_or("active")
                    ))
                })?;
            let execution = AgentExecution {
                agent_id: task.agent_name.clone(),
                model_route: task.model.clone().unwrap_or(definition.model),
                allowed_tools: definition.allowed_tools,
                disable_tools: false,
                system_prompt: definition.system_prompt,
                kb_branch: parent.kb_branch.clone(),
                doc_id: parent.doc_id.clone(),
                session_id: parent.session_id.clone(),
                run_id: subagent_run_id.clone(),
                cancellation: parent.cancellation.clone(),
            };
            let scope = parent_scope.child(subagent_run_id.clone(), task.agent_name.clone());
            let outcome = self
                .run_turn(execution, vec![Message::user(task.task.clone())], scope)
                .await?;
            Ok::<String, RuntimeError>(outcome)
        }
        .await;
        let (status, summary) = match result {
            Ok(_) if parent_scope.cancellation.is_cancelled() => {
                ("cancelled".to_string(), "cancelled".to_string())
            }
            Ok(summary) if summary.trim().is_empty() => {
                ("completed".to_string(), "completed".to_string())
            }
            Ok(summary) => ("completed".to_string(), summary),
            Err(RuntimeError::InvalidRequest(message))
                if parent_scope.cancellation.is_cancelled() || message == "run cancelled" =>
            {
                ("cancelled".to_string(), "cancelled".to_string())
            }
            Err(error) => ("failed".to_string(), error.to_string()),
        };
        parent_scope.send_raw(ModelStreamEvent::SubagentFinished {
            subagent_run_id,
            status: status.clone(),
            summary: summary.clone(),
        });
        SubagentOutcome {
            agent_name: task.agent_name,
            status,
            summary,
        }
    }
}

fn messages_from_request(request: &ModelTurnRequest) -> Vec<Message> {
    let mut messages: Vec<Message> = request.history.iter().map(context_message).collect();
    if !matches!(messages.last(), Some(Message::User { .. })) {
        messages.push(Message::user(request.user_message.clone()));
    }
    messages
}

fn context_message(message: &ContextMessage) -> Message {
    match message.role {
        MessageRole::Assistant => Message::assistant(message.content.clone()),
        // Later system reminders are transient host guidance. Sending them as user content keeps
        // providers that only accept one leading system message happy.
        MessageRole::System => Message::user(message.content.clone()),
        MessageRole::Compaction => Message::user(message.content.clone()),
        MessageRole::User | MessageRole::Tool => Message::user(message.content.clone()),
    }
}

fn build_completion_request(
    execution: &AgentExecution,
    messages: &[Message],
    tools: Vec<ToolDefinition>,
    max_tokens: Option<u64>,
) -> CompletionRequest {
    let mut conversation = messages.to_vec();
    let prompt = conversation
        .pop()
        .unwrap_or_else(|| Message::user(execution.system_prompt.clone()));
    let mut request = CompletionRequest::new(prompt)
        .preamble(runtime_preamble(execution))
        .tools(tools)
        .max_tokens(max_tokens);
    if !conversation.is_empty() {
        request = request.messages(conversation);
    }
    request
}

fn runtime_preamble(execution: &AgentExecution) -> String {
    let mut preamble = execution.system_prompt.clone();
    preamble.push_str("\n\n<runtime_session>\n");
    if let Some(doc_id) = execution.doc_id.as_deref() {
        preamble.push_str(&format!(
            "Bound Quire report doc_id: `{doc_id}`. Use this doc_id for document tools and do not call create_report for this session.\n"
        ));
    } else if !execution.run_id.starts_with("run_sub_") {
        preamble.push_str(
            "No Quire report is bound to this session yet. If the user asks to write or generate a report, choose an appropriate formal report title and call `create_report` first; for pure Q&A, reply directly without creating a report.\n",
        );
    }
    if !execution.run_id.starts_with("run_sub_") {
        preamble.push_str(
            "When the topic of this conversation is clear (especially on the first turn), call `set_session_title` with a concise title.\n",
        );
    }
    preamble.push_str(
        "For complex ECharts charts, call `preview_echarts` first with the complete native ECharts 6.0 `option`. Inspect the rendered screenshot and `lint_issues`, refine with `based_on_preview_id` plus a small `option_patch` only (never resend the full option), and commit the approved chart with `commit_chart` using the final `preview_id`. Do not write chart blocks directly through apply_operations when the preview workflow is available.\n",
    );
    if let Some(branch) = execution.kb_branch.as_deref() {
        preamble.push_str(&format!(
            "Knowledge-base branch for this session: `{branch}`. Agent and skill lookups use this branch.\n"
        ));
    }
    preamble.push_str("</runtime_session>\n");
    preamble
}

async fn handle_stream_event(
    event: &StreamEvent,
    scope: &EventScope,
    previews: &mut PreviewState,
) -> bool {
    match event {
        StreamEvent::Start { part, kind, name } => {
            previews.on_start(part.index(), *kind, name.as_ref());
            false
        }
        StreamEvent::Text { text, .. } => {
            scope.send(ModelStreamEvent::TextDelta(text.clone()));
            true
        }
        StreamEvent::Reasoning { text, .. } => {
            scope.send(ModelStreamEvent::ThinkingDelta(text.clone()));
            false
        }
        StreamEvent::Arguments { part, json } => {
            previews.on_arguments(part.index(), json).await;
            false
        }
        StreamEvent::End { part, content } => {
            if let AssistantContent::ToolCall(call) = content {
                previews.on_end(part.index(), call);
            }
            false
        }
    }
}

async fn request_questionnaire(
    scope: &EventScope,
    execution: &AgentExecution,
    call: ToolCall,
) -> RuntimeResult<()> {
    let provider_call_id = call.id.wire().to_string();
    let tool_call_id = format!("{}:{}", execution.run_id, provider_call_id);
    let arguments = call.function.arguments_value();
    let (input_preview, input_truncated) = preview_json(&arguments);
    scope.send(ModelStreamEvent::ToolCallStarted {
        tool_call_id: tool_call_id.clone(),
        tool_name: "ask_user".into(),
        input_preview,
        truncated: input_truncated,
        input: arguments.clone(),
    });
    let questions_value = arguments.get("questions").cloned().ok_or_else(|| {
        RuntimeError::InvalidRequest("ask_user requires a `questions` array".into())
    })?;
    let questions: Vec<QuestionItem> =
        serde_json::from_value(questions_value).map_err(|error| {
            RuntimeError::InvalidRequest(format!("invalid ask_user questions: {error}"))
        })?;
    if questions.is_empty() {
        return Err(RuntimeError::InvalidRequest(
            "ask_user requires at least one question".into(),
        ));
    }
    scope.send(ModelStreamEvent::QuestionnaireRequested {
        tool_call_id: Some(tool_call_id.clone()),
        questions,
    });
    scope.send(ModelStreamEvent::ToolCallFinished {
        tool_call_id,
        tool_name: "ask_user".into(),
        status: "awaiting_input".into(),
        output_preview: "{\"awaiting_input\":true}".into(),
        truncated: false,
        duration_ms: 0,
        input: arguments,
        output: json!({"awaiting_input": true}),
        error: None,
    });
    Ok(())
}

fn format_invalid_tool_arguments_error(invalid: &str) -> String {
    match serde_json::from_str::<Value>(invalid) {
        Err(err) => {
            format!("invalid JSON in tool arguments (fix syntax and retry): {err}")
        }
        Ok(_) => format!("tool arguments must be a JSON object, got: {invalid}"),
    }
}

async fn execute_tool_call(
    tool_map: &BTreeMap<String, DynamicTool>,
    scope: &EventScope,
    execution: &AgentExecution,
    previews: &mut PreviewState,
    call: ToolCall,
) -> (MessageToolResult, Option<(String, String)>) {
    let provider_call_id = call.id.wire().to_string();
    let tool_call_id = format!("{}:{provider_call_id}", execution.run_id);
    let tool_name = call.function.name.as_str().to_string();
    let mut arguments = call.function.arguments_value();
    if let Some(preview_ids) = previews.take_preview_ids(&provider_call_id) {
        inject_preview_ids(&mut arguments, &preview_ids);
    }
    let (input_preview, input_truncated) = preview_json(&arguments);
    scope.send(ModelStreamEvent::ToolCallStarted {
        tool_call_id: tool_call_id.clone(),
        tool_name: tool_name.clone(),
        input_preview,
        truncated: input_truncated,
        input: arguments.clone(),
    });
    let started = Instant::now();
    let outcome = if let Some(invalid) = call.function.invalid_arguments.clone() {
        let message = format_invalid_tool_arguments_error(&invalid);
        let model_output = json!({
            "kind": "InvalidArgs",
            "error": message.clone(),
            "raw_arguments": invalid,
        });
        Err(ToolExecutionError::invalid_args(message)
            .with_model_output(ToolOutput::json(model_output)))
    } else if let Some(tool) = tool_map.get(&tool_name) {
        tokio::select! {
            _ = execution.cancellation.cancelled() => {
                Err(ToolExecutionError::other("tool call cancelled by user"))
            }
            outcome = tool.run(arguments.clone()) => outcome,
        }
    } else {
        Err(ToolExecutionError::not_found(format!(
            "tool `{tool_name}` is not enabled for this agent"
        )))
    };
    let duration_ms = u64::try_from(started.elapsed().as_millis()).unwrap_or(u64::MAX);
    let (result, output_value, error, status) = match outcome {
        Ok(output) => {
            let value = tool_output_value(&output);
            (
                ExecutedToolResult::success(output),
                value,
                None,
                "completed".to_string(),
            )
        }
        Err(error) => {
            let message = error.to_string();
            let value = json!({
                "error": message,
                "kind": format!("{:?}", error.kind()),
            });
            (
                ExecutedToolResult::failed(error),
                value,
                Some(message),
                "failed".to_string(),
            )
        }
    };
    let linked_doc = if error.is_none() && tool_name == "create_report" {
        output_value
            .get("doc_id")
            .and_then(Value::as_str)
            .map(|doc_id| {
                let title = output_value
                    .get("title")
                    .and_then(Value::as_str)
                    .or_else(|| arguments.get("title").and_then(Value::as_str))
                    .unwrap_or("未命名报告");
                (doc_id.to_string(), title.to_string())
            })
    } else {
        None
    };
    let (output_preview, output_truncated) = preview_json(&output_value);
    scope.send(ModelStreamEvent::ToolCallFinished {
        tool_call_id,
        tool_name,
        status,
        output_preview,
        truncated: output_truncated,
        duration_ms,
        input: arguments,
        output: output_value,
        error,
    });
    (call.answer(&result), linked_doc)
}

fn response_text(response: &CompletionResponse) -> String {
    let parts: Vec<&str> = response
        .choice
        .iter()
        .filter_map(|content| match content {
            AssistantContent::Text(text) => Some(text.text.as_str()),
            _ => None,
        })
        .collect();
    parts.join("\n\n")
}

fn tool_output_value(output: &ToolOutput) -> Value {
    if let Some(value) = output.as_json() {
        return value.clone();
    }
    if let Some(text) = output.as_text() {
        return json!({"text": text});
    }
    serde_json::to_value(output).unwrap_or(Value::Null)
}

fn context_usage(usage: &Usage, context_limit: u64) -> ContextUsage {
    let input_tokens = usage.input_tokens.unwrap_or(0);
    let output_tokens = usage.output_tokens.unwrap_or(0);
    ContextUsage {
        input_tokens,
        output_tokens,
        cached_tokens: usage.cached_input_tokens.unwrap_or(0),
        context_used: usage
            .context_tokens()
            .unwrap_or_else(|| input_tokens.saturating_add(output_tokens)),
        context_limit,
    }
}

fn preview_json(value: &Value) -> (String, bool) {
    let raw = serde_json::to_string(value).unwrap_or_default();
    let mut chars = raw.chars();
    let mut preview: String = chars.by_ref().take(240).collect();
    let truncated = chars.next().is_some();
    if truncated {
        preview.push('…');
    }
    (preview, truncated)
}

fn model_provider_error(error: rig_core::error::ProviderError) -> RuntimeError {
    RuntimeError::Internal(format!("model provider error: {error}"))
}

#[derive(Clone)]
struct PreviewBlock {
    block_id: Option<String>,
    block_type: String,
    title: Option<String>,
    level: Option<u8>,
    text: Option<String>,
    anchor: PreviewAnchor,
}

impl PreviewBlock {
    fn partial(&self, block_id: BlockId) -> PartialBlockPreview {
        PartialBlockPreview {
            block_id: Some(block_id),
            block_type: self.block_type.clone(),
            title: self.title.clone(),
            level: self.level,
            text: self.text.clone(),
        }
    }
}

struct StreamingPreview {
    tool_name: String,
    buffer: String,
    streamers: HashMap<String, ToolPreviewStreamer>,
    preview_ids: BTreeSet<String>,
}

struct PreviewState {
    run_id: String,
    doc_id: Option<String>,
    base_url: String,
    token: SecretString,
    actor: String,
    throttle_ms: u64,
    streams: HashMap<usize, StreamingPreview>,
    previews_by_call: HashMap<String, Vec<String>>,
}

impl PreviewState {
    fn new(execution: &AgentExecution, config: &Config) -> Self {
        Self {
            run_id: execution.run_id.clone(),
            doc_id: execution.doc_id.clone(),
            base_url: config.quire_base_url.clone(),
            token: config.quire_rw_token.clone(),
            actor: execution.agent_id.clone(),
            throttle_ms: config.preview_throttle_ms,
            streams: HashMap::new(),
            previews_by_call: HashMap::new(),
        }
    }

    fn on_start(&mut self, part: usize, kind: PartKind, name: Option<&ToolName>) {
        if kind != PartKind::ToolCall {
            return;
        }
        let Some(name) = name else {
            return;
        };
        if !PREVIEW_TOOLS.contains(&name.as_str()) || self.doc_id.is_none() {
            return;
        }
        self.streams.insert(
            part,
            StreamingPreview {
                tool_name: name.as_str().to_string(),
                buffer: String::new(),
                streamers: HashMap::new(),
                preview_ids: BTreeSet::new(),
            },
        );
    }

    async fn on_arguments(&mut self, part: usize, fragment: &str) {
        let Some(stream) = self.streams.get_mut(&part) else {
            return;
        };
        stream.buffer.push_str(fragment);
        let Some(doc_id) = self.doc_id.clone() else {
            return;
        };
        let arguments = Value::Object(parse_partial_arguments(&stream.buffer));
        let blocks = extract_preview_blocks(&arguments);
        for (index, candidate) in blocks.iter().enumerate() {
            let key = candidate
                .block_id
                .clone()
                .unwrap_or_else(|| format!("auto-{index}"));
            if !stream.streamers.contains_key(&key) {
                let block_id = candidate
                    .block_id
                    .clone()
                    .unwrap_or_else(|| format!("preview{part}{index}"));
                let preview_id = format!("{}:{}:{}:{}", self.run_id, stream.tool_name, part, key);
                stream.streamers.insert(
                    key.clone(),
                    ToolPreviewStreamer::new(
                        &self.base_url,
                        self.token.clone(),
                        &doc_id,
                        &preview_id,
                        &self.actor,
                        &block_id,
                        self.throttle_ms,
                    )
                    .with_anchor(candidate.anchor.clone()),
                );
            }
            if let Some(streamer) = stream.streamers.get_mut(&key) {
                if candidate.anchor != PreviewAnchor::AppendToDoc {
                    streamer.set_anchor(candidate.anchor.clone());
                }
                let block_id = BlockId::parse(
                    candidate
                        .block_id
                        .clone()
                        .unwrap_or_else(|| format!("preview{part}{index}")),
                )
                .unwrap_or_else(|_| BlockId::pending());
                if let Err(error) = streamer.push_block(candidate.partial(block_id)).await {
                    tracing::warn!(%error, tool = %stream.tool_name, "ephemeral preview publish failed");
                } else {
                    stream.preview_ids.insert(preview_id_for(streamer));
                }
            }
        }
    }

    fn on_end(&mut self, part: usize, call: &ToolCall) {
        let Some(stream) = self.streams.remove(&part) else {
            return;
        };
        if !stream.preview_ids.is_empty() {
            self.previews_by_call.insert(
                call.id.wire().to_string(),
                stream.preview_ids.into_iter().collect(),
            );
        }
    }

    fn take_preview_ids(&mut self, call_id: &str) -> Option<Vec<String>> {
        self.previews_by_call.remove(call_id)
    }
}

fn preview_id_for(streamer: &ToolPreviewStreamer) -> String {
    streamer.preview_id().to_string()
}

fn extract_preview_blocks(arguments: &Value) -> Vec<PreviewBlock> {
    let mut blocks = Vec::new();
    let mut seen = BTreeSet::new();
    collect_preview_blocks(
        arguments,
        0,
        &PreviewAnchor::AppendToDoc,
        &mut blocks,
        &mut seen,
    );
    blocks
}

fn infer_operation_anchor(
    object: &serde_json::Map<String, Value>,
    inherited: &PreviewAnchor,
) -> PreviewAnchor {
    let op_name = object.get("op").and_then(Value::as_str);
    if let Some(section_id) = object
        .get("section_id")
        .and_then(Value::as_str)
        .and_then(|raw| BlockId::parse(raw.to_string()).ok())
    {
        return PreviewAnchor::AppendToSection { section_id };
    }
    if op_name == Some("replace_block")
        && let Some(block_id) = object
            .get("block_id")
            .and_then(Value::as_str)
            .and_then(|raw| BlockId::parse(raw.to_string()).ok())
    {
        return PreviewAnchor::ReplaceBlock { block_id };
    }
    if let Some(block_id) = object
        .get("after_id")
        .or_else(|| object.get("after_block_id"))
        .and_then(Value::as_str)
        .and_then(|raw| BlockId::parse(raw.to_string()).ok())
    {
        return PreviewAnchor::AfterBlock { block_id };
    }
    if let Some(block_id) = object
        .get("before_id")
        .or_else(|| object.get("before_block_id"))
        .and_then(Value::as_str)
        .and_then(|raw| BlockId::parse(raw.to_string()).ok())
    {
        return PreviewAnchor::BeforeBlock { block_id };
    }
    inherited.clone()
}

fn collect_preview_blocks(
    value: &Value,
    depth: usize,
    anchor: &PreviewAnchor,
    blocks: &mut Vec<PreviewBlock>,
    seen: &mut BTreeSet<String>,
) {
    if depth > 8 {
        return;
    }
    match value {
        Value::Object(object) => {
            let next_anchor = infer_operation_anchor(object, anchor);
            if let Some(block) = preview_block(object, &next_anchor) {
                let key = block
                    .block_id
                    .clone()
                    .unwrap_or_else(|| format!("{}:{}", block.block_type, blocks.len()));
                if seen.insert(key) {
                    blocks.push(block);
                }
            }
            for child in object.values() {
                collect_preview_blocks(child, depth + 1, &next_anchor, blocks, seen);
            }
        }
        Value::Array(values) => {
            for child in values {
                collect_preview_blocks(child, depth + 1, anchor, blocks, seen);
            }
        }
        _ => {}
    }
}

fn preview_block(
    object: &serde_json::Map<String, Value>,
    anchor: &PreviewAnchor,
) -> Option<PreviewBlock> {
    let text = object.get("text").and_then(Value::as_str);
    let title = object.get("title").and_then(Value::as_str);
    if text.is_none() && title.is_none() {
        return None;
    }
    let inferred_type = if object.contains_key("level") {
        "heading"
    } else if object.contains_key("rows") || object.contains_key("columns") {
        "table"
    } else if object.contains_key("option") {
        "echarts"
    } else {
        "paragraph"
    };
    let block_type = object
        .get("type")
        .and_then(Value::as_str)
        .unwrap_or(inferred_type);
    if !matches!(
        block_type,
        "heading" | "paragraph" | "quote" | "table" | "echarts" | "custom_html"
    ) {
        return None;
    }
    let block_id = object
        .get("id")
        .and_then(Value::as_str)
        .filter(|value| BlockId::parse((*value).to_string()).is_ok())
        .map(str::to_string);
    let level = object
        .get("level")
        .and_then(Value::as_u64)
        .and_then(|value| u8::try_from(value).ok());
    Some(PreviewBlock {
        block_id,
        block_type: block_type.to_string(),
        title: title.map(str::to_string),
        level,
        text: text.map(str::to_string),
        anchor: anchor.clone(),
    })
}

fn inject_preview_ids(arguments: &mut Value, preview_ids: &[String]) {
    if preview_ids.is_empty() {
        return;
    }
    let Some(object) = arguments.as_object_mut() else {
        return;
    };
    let mut merged: Vec<String> = object
        .get("preview_ids")
        .and_then(Value::as_array)
        .map(|values| {
            values
                .iter()
                .filter_map(Value::as_str)
                .map(str::to_string)
                .collect()
        })
        .unwrap_or_default();
    for preview_id in preview_ids {
        if !merged.iter().any(|existing| existing == preview_id) {
            merged.push(preview_id.clone());
        }
    }
    object.insert("preview_ids".to_string(), json!(merged));
}

#[cfg(test)]
mod tests {
    use super::*;
    use rig_core::message::ToolFunction;
    use std::sync::atomic::{AtomicUsize, Ordering};

    fn test_tool(name: &str) -> DynamicTool {
        DynamicTool::new(
            ToolName::new(name).unwrap(),
            "test tool",
            json!({}),
            move |_args| Box::pin(async move { Ok(ToolOutput::text("ok")) }),
        )
    }

    async fn execute_invalid_arguments_call(raw: &str) -> (MessageToolResult, Value, usize) {
        let calls = Arc::new(AtomicUsize::new(0));
        let tool_calls = Arc::clone(&calls);
        let tool = DynamicTool::new(
            ToolName::new("probe").unwrap(),
            "Probe used to prove invalid arguments never execute.",
            json!({}),
            move |_args| {
                let tool_calls = Arc::clone(&tool_calls);
                Box::pin(async move {
                    tool_calls.fetch_add(1, Ordering::SeqCst);
                    Ok(ToolOutput::text("executed"))
                })
            },
        );
        let mut tool_map = BTreeMap::new();
        tool_map.insert("probe".to_string(), tool);

        let (sender, mut receiver) = mpsc::unbounded_channel();
        let scope = EventScope::top(sender, CancellationToken::new());
        let execution = AgentExecution {
            agent_id: "main".into(),
            model_route: "stub/echo".into(),
            allowed_tools: vec!["probe".into()],
            disable_tools: false,
            system_prompt: "test".into(),
            kb_branch: Some("main".into()),
            doc_id: None,
            session_id: "sess_test".into(),
            run_id: "run_test".into(),
            cancellation: CancellationToken::new(),
        };
        let mut previews = PreviewState {
            run_id: execution.run_id.clone(),
            doc_id: None,
            base_url: String::new(),
            token: SecretString::from(String::new()),
            actor: "test".into(),
            throttle_ms: 0,
            streams: HashMap::new(),
            previews_by_call: HashMap::new(),
        };
        let call = ToolCall::from_wire(
            "call_invalid",
            ToolFunction::parse(ToolName::new("probe").unwrap(), raw),
        );

        let (result, linked_doc) =
            execute_tool_call(&tool_map, &scope, &execution, &mut previews, call).await;
        assert!(linked_doc.is_none());

        let mut finished_output = Value::Null;
        while let Ok(event) = receiver.try_recv() {
            if let Ok(ModelStreamEvent::ToolCallFinished { output, .. }) = event {
                finished_output = output;
            }
        }
        (result, finished_output, calls.load(Ordering::SeqCst))
    }

    #[tokio::test]
    async fn invalid_tool_arguments_never_execute_the_tool() {
        for raw in [r#"{"total":"13%}"#, r#"["a"]"#] {
            let (result, finished_output, calls) = execute_invalid_arguments_call(raw).await;
            assert_eq!(calls, 0);
            assert!(result.is_error);

            let content = result
                .content
                .iter()
                .find_map(|entry| entry.as_json())
                .expect("structured invalid-arguments feedback");
            assert_eq!(
                content.get("kind").and_then(Value::as_str),
                Some("InvalidArgs")
            );
            assert_eq!(
                content.get("raw_arguments").and_then(Value::as_str),
                Some(raw)
            );
            assert!(
                content
                    .get("error")
                    .and_then(Value::as_str)
                    .is_some_and(|error| !error.trim().is_empty())
            );
            assert_eq!(
                finished_output.get("kind").and_then(Value::as_str),
                Some("InvalidArgs")
            );
        }
    }

    #[test]
    fn wildcard_allow_list_enables_every_builtin_tool() {
        let wildcard = vec!["*".to_string()];
        for tool in BUILTIN_TOOLS {
            assert!(
                allow_list_matches(&wildcard, tool),
                "wildcard should enable built-in tool `{tool}`"
            );
        }
        assert!(allow_list_matches(&wildcard, "apply_operations"));
    }

    #[test]
    fn ask_user_schema_exposes_questions_array() {
        let parameters = ASK_USER_TOOL.parameters();
        let properties = parameters
            .get("properties")
            .and_then(Value::as_object)
            .expect("ask_user schema should have properties");
        assert!(properties.contains_key("questions"));
        assert!(!properties.contains_key("_questions"));
        assert_eq!(
            parameters.get("required"),
            Some(&json!(["questions"])),
            "ask_user must require the `questions` array"
        );
    }

    #[test]
    fn exact_allow_list_stays_scoped() {
        let allowed = vec!["spawn_subagents".to_string(), "lint_document".to_string()];
        assert!(allow_list_matches(&allowed, "spawn_subagents"));
        assert!(allow_list_matches(&allowed, "lint_document"));
        assert!(!allow_list_matches(&allowed, "ask_user"));
        assert!(!allow_list_matches(&[], "spawn_subagents"));
    }

    #[test]
    fn wildcard_allow_list_supports_prefix_suffix_and_multi_segment_patterns() {
        let allowed = vec![
            "subjectcloud_*".to_string(),
            "shuangyou_intel_*".to_string(),
            "*_detail".to_string(),
            "panorama_report_*_query".to_string(),
        ];
        assert!(allow_list_matches(&allowed, "subjectcloud_overview"));
        assert!(allow_list_matches(&allowed, "shuangyou_intel_list"));
        assert!(allow_list_matches(&allowed, "subjectcloud_talent_detail"));
        assert!(allow_list_matches(&allowed, "panorama_report_batch_query"));
        assert!(!allow_list_matches(&allowed, "shuangyou_profile"));
        assert!(!allow_list_matches(&allowed, "panorama_report_trend"));
    }

    #[test]
    fn warn_missing_allowed_tools_expands_wildcards_instead_of_literal_lookup() {
        let tool_map = BTreeMap::from([
            (
                "subjectcloud_detail".to_string(),
                test_tool("subjectcloud_detail"),
            ),
            (
                "subjectcloud_overview".to_string(),
                test_tool("subjectcloud_overview"),
            ),
            (
                "shuangyou_profile".to_string(),
                test_tool("shuangyou_profile"),
            ),
        ]);

        let valid = vec![
            "subjectcloud_*".to_string(),
            "server_a:subjectcloud_*".to_string(),
            "subjectcloud_detail".to_string(),
            "*".to_string(),
        ];
        assert!(
            warn_missing_allowed_tools("agent", &valid, &tool_map).is_empty(),
            "valid wildcards must not be reported as missing tools"
        );

        let missing = warn_missing_allowed_tools(
            "agent",
            &[
                "panorama_report_*".to_string(),
                "shuangyou_intel_*".to_string(),
            ],
            &tool_map,
        );
        assert_eq!(
            missing,
            vec!["panorama_report_*", "shuangyou_intel_*"],
            "only patterns with no matching assembled tool should warn"
        );
    }

    #[test]
    fn sandbox_tool_schema_exposes_code_and_optional_limits() {
        let parameters = SANDBOX_EXECUTE_TOOL.parameters();
        let properties = parameters
            .get("properties")
            .and_then(Value::as_object)
            .expect("sandbox_execute schema should have properties");
        assert!(properties.contains_key("language"));
        assert!(properties.contains_key("code"));
        assert!(properties.contains_key("cwd"));
        assert!(properties.contains_key("timeout_seconds"));
        assert_eq!(
            parameters.get("required"),
            Some(&json!(["language", "code"]))
        );
    }

    #[test]
    fn chart_preview_tools_enforce_the_no_repeat_option_contract() {
        let preview = PREVIEW_ECHARTS_TOOL.parameters();
        let preview_properties = preview
            .get("properties")
            .and_then(Value::as_object)
            .expect("preview_echarts schema should have properties");
        assert!(preview_properties.contains_key("option"));
        assert!(preview_properties.contains_key("option_patch"));
        assert!(preview_properties.contains_key("based_on_preview_id"));

        let commit = COMMIT_CHART_TOOL.parameters();
        let commit_properties = commit
            .get("properties")
            .and_then(Value::as_object)
            .expect("commit_chart schema should have properties");
        assert!(commit_properties.contains_key("preview_id"));
        assert!(commit_properties.contains_key("doc_id"));
        assert!(commit_properties.contains_key("block_id"));
        assert!(!commit_properties.contains_key("option"));
        assert!(!commit_properties.contains_key("option_patch"));
    }

    #[test]
    fn bounded_tool_text_preserves_utf8_boundaries() {
        let (text, truncated) = bounded_tool_text("好".repeat(MAX_TOOL_OUTPUT_BYTES));
        assert!(truncated);
        assert!(text.is_char_boundary(text.len()));
        assert!(text.len() <= MAX_TOOL_OUTPUT_BYTES);
    }

    #[test]
    fn preview_throttle_respects_the_interval() {
        let mut throttle = PreviewThrottle::new(60);
        let start = Instant::now();
        assert!(throttle.should_emit_at(start));
        assert!(!throttle.should_emit_at(start + Duration::from_millis(20)));
        assert!(throttle.should_emit_at(start + Duration::from_millis(61)));
    }

    #[tokio::test]
    async fn subagents_run_concurrently_and_preserve_outcomes() {
        let tasks = vec![
            SubagentTask {
                agent_name: "a".into(),
                display_name: None,
                model: None,
                task: "one".into(),
                section_id: None,
            },
            SubagentTask {
                agent_name: "b".into(),
                display_name: None,
                model: Some("subagent/model-b".into()),
                task: "two".into(),
                section_id: Some("ch2".into()),
            },
        ];
        let outcomes = spawn_subagents(tasks, |task| async move {
            SubagentOutcome {
                agent_name: task.agent_name,
                status: "completed".into(),
                summary: task.task,
            }
        })
        .await
        .unwrap();
        assert_eq!(outcomes.len(), 2);
    }

    #[test]
    fn partial_apply_operations_arguments_produce_block_previews() {
        let raw = r#"{"ops":[{"type":"append","blocks":[{"id":"q3-revenue","type":"paragraph","text":"Revenue grew"#;
        let arguments = Value::Object(parse_partial_arguments(raw));
        let blocks = extract_preview_blocks(&arguments);
        assert_eq!(blocks.len(), 1);
        let block = blocks.first().expect("preview block");
        assert_eq!(block.block_id.as_deref(), Some("q3-revenue"));
        assert_eq!(block.block_type, "paragraph");
        assert_eq!(block.text.as_deref(), Some("Revenue grew"));
    }

    #[test]
    fn preview_ids_are_merged_into_tool_arguments() {
        let mut arguments = json!({"doc_id": "report", "preview_ids": ["old"]});
        inject_preview_ids(&mut arguments, &["new".into(), "old".into()]);
        let ids = arguments
            .get("preview_ids")
            .and_then(Value::as_array)
            .expect("preview ids");
        assert_eq!(ids.len(), 2);
        assert_eq!(ids.first().and_then(Value::as_str), Some("old"));
        assert_eq!(ids.last().and_then(Value::as_str), Some("new"));
    }

    #[test]
    fn usage_carries_real_tokens_and_context_limit() {
        let usage = Usage::new()
            .input_tokens(120_u64)
            .output_tokens(30_u64)
            .cached_input_tokens(40_u64);
        let mapped = context_usage(&usage, 64_000);
        assert_eq!(mapped.input_tokens, 120);
        assert_eq!(mapped.output_tokens, 30);
        assert_eq!(mapped.cached_tokens, 40);
        assert_eq!(mapped.context_used, 150);
        assert_eq!(mapped.context_limit, 64_000);
    }

    #[tokio::test]
    async fn ask_user_emits_questionnaire_and_pauses_the_run() {
        use rig_core::message::ToolFunction;

        let (sender, mut receiver) = mpsc::unbounded_channel();
        let scope = EventScope::top(sender, CancellationToken::new());
        let execution = AgentExecution {
            agent_id: "main".into(),
            model_route: "stub/echo".into(),
            allowed_tools: vec!["ask_user".into()],
            disable_tools: false,
            system_prompt: "test".into(),
            kb_branch: Some("main".into()),
            doc_id: Some("report".into()),
            session_id: "sess_test".into(),
            run_id: "run_test".into(),
            cancellation: CancellationToken::new(),
        };
        let call = ToolCall::from_wire(
            "call_ask",
            ToolFunction::new(
                ToolName::new("ask_user").unwrap(),
                json!({
                    "questions": [{
                        "id": "scope",
                        "title": "Which scope?",
                        "multiple": false,
                        "required": true,
                        "options": [{"id": "q3", "label": "Q3"}]
                    }]
                }),
            ),
        );
        request_questionnaire(&scope, &execution, call)
            .await
            .unwrap();
        assert!(matches!(
            receiver.recv().await.unwrap().unwrap(),
            ModelStreamEvent::ToolCallStarted { .. }
        ));
        let requested = receiver.recv().await.unwrap().unwrap();
        match requested {
            ModelStreamEvent::QuestionnaireRequested { questions, .. } => {
                assert_eq!(questions.len(), 1);
                assert_eq!(
                    questions.first().map(|question| question.id.as_str()),
                    Some("scope")
                );
            }
            other => panic!("expected questionnaire, got {other:?}"),
        }
        assert!(matches!(
            receiver.recv().await.unwrap().unwrap(),
            ModelStreamEvent::ToolCallFinished {
                status,
                ..
            } if status == "awaiting_input"
        ));
    }
}
