//! Built-in tool definitions, schemas and credential/URL redaction helpers.

use std::{collections::BTreeSet, sync::Arc};

use base64::{Engine as _, engine::general_purpose::STANDARD as BASE64};
use quire_lint::lint_echarts_block;
use quire_protocol::{
    Block, BlockId, BlockKind, EchartsBlock, PatchOp, chart_data_tables_for_block,
    chart_requirements, merge_chart_option_patch, short_id,
};
use quire_session_protocol::QuestionItem;
#[cfg(test)]
use rig::tool::Tool as _;
use rig::tool::{rmcp::McpMeta, server::ToolServer};
use rig::{
    message::{ImageMediaType, ToolResultContent},
    rig_tool,
    tool::{ToolContext, ToolExecutionError, ToolOutput},
};
use serde_json::{Value, json};
use tracing::warn;

use crate::{
    mcp_registry::tool_pattern_matches,
    model_stream::ModelStreamEvent,
    sandbox::{SandboxExecRequest, SandboxLease},
    service::{SANDBOX_ATTACHMENT_GET_TTL_SECS, safe_attachment_filename, safe_attachment_id},
    store::SessionPatch,
};

use super::{
    AgentExecution, RigModelCore, SharedDocId, StreamEventSender,
    context::{AskUserRequestedMeta, QuireTurnMeta},
    subagents::RunSubagentsTool,
};

const REPORT_PREVIEW_CAPTION: &str = "Rendered screenshot from preview_report. Inspect visual layout, spacing, tables, and charts alongside the diagnostic JSON.";
const CHART_PREVIEW_CAPTION: &str = "Rendered ECharts screenshot from preview_echarts. Verify the chart type, series data, axis labels, legend, tooltip/static labels, colors, clipping/overflow, and that the visual matches fallback_summary. Refine with preview_echarts + based_on_preview_id + option_patch only, then call commit_chart once it looks right.";

fn image_media_type(mime_type: &str) -> ImageMediaType {
    match mime_type {
        "image/jpeg" => ImageMediaType::JPEG,
        "image/webp" => ImageMediaType::WEBP,
        _ => ImageMediaType::PNG,
    }
}

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
    "sandbox_mount_attachment",
];

/// Return configured patterns that match no assembled tool.
///
/// Wildcards are expanded against the final tool map rather than looked up as
/// literal names. A scoped pattern such as `server_a:subjectcloud_*` is checked
/// against the tool-name portion because assembled MCP tools keep bare names.
fn missing_allowed_tool_patterns(
    allowed_tools: &[String],
    available_tools: &BTreeSet<String>,
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
            available_tools
                .iter()
                .any(|name| tool_pattern_matches(effective, name))
        } else {
            available_tools.contains(effective)
        };
        if !available {
            missing.push(trimmed.to_string());
        }
    }
    missing
}

pub(super) fn warn_missing_allowed_tools(
    agent_id: &str,
    allowed_tools: &[String],
    available_tools: &BTreeSet<String>,
) -> Vec<String> {
    let missing = missing_allowed_tool_patterns(allowed_tools, available_tools);
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

pub(super) struct ToolBuildContext {
    pub(super) execution: AgentExecution,
    pub(super) scope: StreamEventSender,
    pub(super) engine: Arc<RigModelCore>,
    pub(super) sandbox: Arc<SandboxLease>,
}

pub(super) struct BuiltInTools {
    pub(super) server: ToolServer,
    pub(super) names: BTreeSet<String>,
    pub(super) runtime: Arc<RuntimeToolScope>,
}

pub(super) struct RuntimeToolScope {
    pub(super) engine: Arc<RigModelCore>,
    pub(super) event_scope: StreamEventSender,
    sandbox: Arc<SandboxLease>,
    pub(super) cancellation: tokio_util::sync::CancellationToken,
    /// Latest bound report ID, shared across the turn so subagents spawned
    /// after a lazy `create_report` inherit the new document.
    pub(super) doc_id: SharedDocId,
}

pub(super) fn runtime_tool_scope(
    context: &ToolContext,
) -> Result<Arc<RuntimeToolScope>, ToolExecutionError> {
    context
        .scope::<RuntimeToolScope>()
        .ok_or_else(|| ToolExecutionError::other("missing runtime tool context"))
}

pub(super) fn quire_turn_meta(context: &ToolContext) -> Result<QuireTurnMeta, ToolExecutionError> {
    context
        .require::<QuireTurnMeta>()
        .map_err(|error| ToolExecutionError::other(error.to_string()))
}

pub(super) fn tool_other(error: impl std::fmt::Display) -> ToolExecutionError {
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

#[rig_tool(
    name = "sandbox_mount_attachment",
    description = "Mount an attachment uploaded to this session into the run's sandbox at /workspace/attachments/{attachment_id}/{filename}. Returns the sandbox path and attachment metadata. Download credentials, signed URLs and storage internals are never returned."
)]
async fn sandbox_mount_attachment_tool(
    #[rig(context)] context: &mut ToolContext,
    attachment_id: String,
) -> Result<Value, ToolExecutionError> {
    let runtime = runtime_tool_scope(context)?;
    let turn = quire_turn_meta(context)?;
    let attachment_id = safe_attachment_id(&attachment_id).map_err(tool_other)?;
    let stored = runtime
        .engine
        .kb
        .store()
        .get_session_attachment(&turn.session_id, attachment_id)
        .await
        .map_err(tool_other)?;
    let attachment_id = safe_attachment_id(&stored.attachment_id).map_err(tool_other)?;
    let filename = safe_attachment_filename(&stored.filename).map_err(tool_other)?;
    let directory = format!("/workspace/attachments/{attachment_id}");
    let target_path = format!("{directory}/{filename}");
    let sandbox = runtime.sandbox.get_or_create().await.map_err(tool_other)?;

    match stored.storage_provider.as_str() {
        "oss" => {
            let object_key = stored.storage_path.as_str();
            if !runtime
                .engine
                .config
                .oss
                .owns_object_key(&turn.session_id, object_key)
            {
                return Err(ToolExecutionError::other(
                    "attachment object key does not belong to this session",
                ));
            }
            let signed_url = runtime
                .engine
                .config
                .oss
                .presign_get_for_sandbox(object_key, SANDBOX_ATTACHMENT_GET_TTL_SECS)
                .map_err(tool_other)?;
            let command = format!(
                "set -euo pipefail\n\
                 if ! mkdir -p {directory} 2>/dev/null; then\n\
                   sudo -n mkdir -p {directory}\n\
                   sudo -n chown \"$(id -u):$(id -g)\" /workspace\n\
                   sudo -n chown -R \"$(id -u):$(id -g)\" {directory}\n\
                 fi\n\
                 curl -fsSL --retry 2 --connect-timeout 15 --max-time 120 -o {target} {url}",
                directory = shell_quote(&directory),
                target = shell_quote(&target_path),
                url = shell_quote(&signed_url),
            );
            let result = sandbox
                .exec_code(SandboxExecRequest {
                    language: "bash".into(),
                    code: command,
                    cwd: None,
                    timeout_seconds: Some(150),
                })
                .await
                .map_err(tool_other)?;
            if result.exit_code != 0 {
                return Err(ToolExecutionError::other(format!(
                    "failed to download attachment from OSS (exit code {}): {}",
                    result.exit_code,
                    sanitize_oss_download_error(&result.stderr, &signed_url)
                )));
            }
        }
        "local" => {
            let bytes = tokio::fs::read(&stored.storage_path)
                .await
                .map_err(|error| {
                    tool_other(format!("failed to read local attachment file: {error}"))
                })?;
            sandbox
                .upload_file(&target_path, &bytes)
                .await
                .map_err(tool_other)?;
        }
        provider => {
            return Err(ToolExecutionError::other(format!(
                "unsupported attachment storage provider `{provider}`"
            )));
        }
    }

    Ok(json!({
        "sandbox_id": sandbox.id(),
        "attachment_id": attachment_id,
        "path": target_path,
        "filename": filename,
        "mime_type": stored.mime_type,
        "size_bytes": stored.size_bytes,
    }))
}

fn shell_quote(value: &str) -> String {
    let mut quoted = String::with_capacity(value.len().saturating_add(2));
    quoted.push('\'');
    for character in value.chars() {
        if character == '\'' {
            quoted.push_str("'\\''");
        } else {
            quoted.push(character);
        }
    }
    quoted.push('\'');
    quoted
}

/// Remove any URL and signed query values before an OSS download error can
/// reach the model, the persisted tool call or a RunTrace preview.
fn sanitize_oss_download_error(stderr: &str, signed_url: &str) -> String {
    let mut sanitized = stderr.replace(signed_url, "[REDACTED_OSS_URL]");
    sanitized = redact_urls(&sanitized);
    for parameter in [
        "x-oss-signature=",
        "x-oss-credential=",
        "x-oss-security-token=",
    ] {
        sanitized = redact_query_parameter(&sanitized, parameter);
    }
    let bounded: String = sanitized.trim().chars().take(2_000).collect();
    if bounded.is_empty() {
        "OSS download command failed without diagnostic output".into()
    } else {
        bounded
    }
}

fn redact_urls(text: &str) -> String {
    let mut output = String::with_capacity(text.len());
    let mut rest = text;
    loop {
        let position = match (rest.find("https://"), rest.find("http://")) {
            (Some(left), Some(right)) => left.min(right),
            (Some(left), None) => left,
            (None, Some(right)) => right,
            (None, None) => break,
        };
        let (before, from_url) = rest.split_at(position);
        output.push_str(before);
        output.push_str("[REDACTED_OSS_URL]");
        let end = from_url
            .find(|character: char| {
                character.is_whitespace() || matches!(character, '\'' | '"' | ')' | ']' | '>' | '<')
            })
            .unwrap_or(from_url.len());
        if end == 0 {
            break;
        }
        rest = from_url.get(end..).unwrap_or_default();
    }
    output.push_str(rest);
    output
}

fn redact_query_parameter(text: &str, parameter: &str) -> String {
    let mut output = String::with_capacity(text.len());
    let mut rest = text;
    while let Some(position) = rest.find(parameter) {
        let (before, from_parameter) = rest.split_at(position);
        output.push_str(before);
        output.push_str("[REDACTED_OSS_QUERY]");
        let value_start = parameter.len();
        let end = from_parameter
            .get(value_start..)
            .and_then(|value| {
                value.find(|character: char| {
                    character.is_whitespace()
                        || matches!(character, '&' | '\'' | '"' | ')' | ']' | '>' | '<')
                })
            })
            .map(|offset| value_start.saturating_add(offset))
            .unwrap_or(from_parameter.len());
        if end <= value_start {
            rest = from_parameter.get(value_start..).unwrap_or_default();
            continue;
        }
        rest = from_parameter.get(end..).unwrap_or_default();
    }
    output.push_str(rest);
    output
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
    let mut content = vec![ToolResultContent::json(result.to_diagnostics_json())];
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
    let diagnostics = json!({
        "preview_id": preview_id,
        "preview_url": preview_url,
        "lint_issues": lint_issues,
        "lint_summary": lint_summary,
        "image_included": image_included,
        "preview_error": preview_error,
    });
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
    let mcp_meta = context
        .require::<McpMeta>()
        .map_err(|error| ToolExecutionError::other(error.to_string()))?;
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

    let client = runtime
        .engine
        .mcp
        .shared_client_for_server("quire-report")
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
                .call_tool_with_meta(
                    "get_document_digest",
                    json!({ "doc_id": doc_id }),
                    mcp_meta.clone(),
                )
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
        .call_tool_with_meta(
            "apply_operations",
            json!({
                "doc_id": doc_id,
                "expected_revision": expected_revision,
                "summary": summary,
                "ops": [serde_json::to_value(&op).map_err(tool_other)?],
            }),
            mcp_meta,
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
    let turn = quire_turn_meta(context)?;
    let skill = runtime
        .engine
        .kb
        .get_skill_on(turn.kb_branch.as_deref(), &name)
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
    let turn = quire_turn_meta(context)?;
    let definition = runtime
        .engine
        .kb
        .get_skill_on(turn.kb_branch.as_deref(), &skill)
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
    description = "Report the current work stage to the user interface. Use it when you want the user to see what you are working on, especially during longer multi-step tasks.",
    params(
        title = "Short Chinese status phrase, ideally at most 16 characters.",
        detail = "Optional clarification of the current focus or blocker.",
        section_id = "Optional `block_id` of the report heading currently being worked on."
    )
)]
async fn describe_task_tool(
    #[rig(context)] context: &mut ToolContext,
    title: String,
    detail: Option<String>,
    section_id: Option<String>,
) -> Result<Value, ToolExecutionError> {
    let runtime = runtime_tool_scope(context)?;
    let title = title.trim().to_string();
    if title.is_empty() {
        return Err(ToolExecutionError::invalid_args(
            "`title` must be a non-empty status phrase",
        ));
    }
    runtime
        .event_scope
        .send(ModelStreamEvent::TaskFocus {
            title: title.clone(),
            detail: detail.clone(),
            section_id: section_id.clone(),
        })
        .await;
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
    let turn = quire_turn_meta(context)?;
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
            &turn.session_id,
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
        })
        .await;
    Ok(json!({
        "session_id": turn.session_id,
        "title": title,
    }))
}

#[rig_tool(
    name = "ask_user",
    description = "Emit a one-shot structured questionnaire and end the current turn. Any sibling tool calls in the same model turn are skipped without execution."
)]
fn ask_user_tool(
    #[rig(context)] context: &mut ToolContext,
    questions: Vec<QuestionItem>,
) -> Result<Value, ToolExecutionError> {
    if questions.is_empty() {
        return Err(ToolExecutionError::invalid_args(
            "`ask_user` requires at least one question item",
        ));
    }
    for item in &questions {
        if item.id.trim().is_empty() || item.title.trim().is_empty() {
            return Err(ToolExecutionError::invalid_args(
                "every question item must have a non-empty `id` and `title`",
            ));
        }
        if item.options.is_empty() {
            return Err(ToolExecutionError::invalid_args(format!(
                "question `{}` must provide at least one option",
                item.id
            )));
        }
    }
    context
        .insert_result(AskUserRequestedMeta {
            questions: questions.clone(),
        })
        .map_err(|error| ToolExecutionError::other(error.to_string()))?;
    Ok(json!({
        "status": "awaiting_input",
        "question_count": questions.len()
    }))
}

#[rig_tool(
    name = "list_subagents",
    description = "List available subagent names and descriptions on this branch."
)]
async fn list_subagents_tool(
    #[rig(context)] context: &mut ToolContext,
) -> Result<Value, ToolExecutionError> {
    let runtime = runtime_tool_scope(context)?;
    let turn = quire_turn_meta(context)?;
    let agents = runtime
        .engine
        .kb
        .list_agents_on(turn.kb_branch.as_deref())
        .await
        .map_err(tool_other)?;
    let subagents: Vec<Value> = agents
        .into_iter()
        .filter(|agent| agent.enabled && !agent.is_default && agent.name != turn.agent_id)
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

impl RigModelCore {
    pub(super) fn build_tools(&self, context: ToolBuildContext) -> BuiltInTools {
        let ToolBuildContext {
            execution,
            scope,
            engine,
            sandbox,
        } = context;
        let runtime = Arc::new(RuntimeToolScope {
            engine,
            event_scope: scope,
            sandbox,
            cancellation: execution.cancellation.clone(),
            doc_id: Arc::clone(&execution.doc_id),
        });
        let mut server = ToolServer::new();
        let mut names = BTreeSet::new();
        macro_rules! register {
            ($name:literal, $enabled:expr, $tool:expr) => {
                if $enabled {
                    names.insert($name.to_string());
                    server = server.tool($tool);
                }
            };
        }

        register!(
            "preview_report",
            allow_list_matches(&execution.allowed_tools, "preview_report"),
            PreviewReportTool
        );
        register!(
            "preview_echarts",
            allow_list_matches(&execution.allowed_tools, "preview_echarts"),
            PreviewEchartsTool
        );
        register!(
            "commit_chart",
            allow_list_matches(&execution.allowed_tools, "commit_chart"),
            CommitChartTool
        );
        register!(
            "sandbox_execute",
            allow_list_matches(&execution.allowed_tools, "sandbox_execute"),
            SandboxExecuteTool
        );
        register!(
            "sandbox_read_file",
            allow_list_matches(&execution.allowed_tools, "sandbox_read_file"),
            SandboxReadFileTool
        );
        register!(
            "sandbox_write_file",
            allow_list_matches(&execution.allowed_tools, "sandbox_write_file"),
            SandboxWriteFileTool
        );
        register!(
            "sandbox_mount_attachment",
            allow_list_matches(&execution.allowed_tools, "sandbox_mount_attachment"),
            SandboxMountAttachmentTool
        );
        register!(
            "load_skill",
            allow_list_matches(&execution.allowed_tools, "load_skill"),
            LoadSkillTool
        );
        register!(
            "read_skill_resource",
            allow_list_matches(&execution.allowed_tools, "read_skill_resource"),
            ReadSkillResourceTool
        );
        register!(
            "describe_task",
            allow_list_matches(&execution.allowed_tools, "describe_task"),
            DescribeTaskTool
        );
        register!(
            "set_session_title",
            !execution.run_id.starts_with("run_sub_")
                && (allow_list_matches(&execution.allowed_tools, "set_session_title")
                    || allow_list_matches(&execution.allowed_tools, "describe_task")),
            SetSessionTitleTool
        );
        register!(
            "ask_user",
            !execution.run_id.starts_with("run_sub_")
                && allow_list_matches(&execution.allowed_tools, "ask_user"),
            AskUserTool
        );
        register!(
            "list_subagents",
            allow_list_matches(&execution.allowed_tools, "list_subagents")
                || (!execution.run_id.starts_with("run_sub_")
                    && allow_list_matches(&execution.allowed_tools, "spawn_subagents")),
            ListSubagentsTool
        );
        register!(
            "spawn_subagents",
            allow_list_matches(&execution.allowed_tools, "spawn_subagents"),
            RunSubagentsTool
        );
        BuiltInTools {
            server,
            names,
            runtime,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

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
    fn describe_task_schema_advertises_status_and_parameters() {
        let description = DESCRIBE_TASK_TOOL.description();
        assert!(description.contains("current work stage"));
        assert!(description.contains("user interface"));

        let parameters = DESCRIBE_TASK_TOOL.parameters();
        let properties = parameters
            .get("properties")
            .and_then(Value::as_object)
            .expect("describe_task schema should have properties");
        for name in ["title", "detail", "section_id"] {
            assert!(properties.contains_key(name), "missing `{name}`");
        }
        let title = properties
            .get("title")
            .and_then(|value| value.get("description"))
            .and_then(Value::as_str)
            .expect("title description");
        assert!(title.contains("16"));
        assert_eq!(parameters.get("required"), Some(&json!(["title"])));
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
        let tool_map = BTreeSet::from([
            "subjectcloud_detail".to_string(),
            "subjectcloud_overview".to_string(),
            "shuangyou_profile".to_string(),
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
    fn sandbox_mount_attachment_schema_requires_attachment_id() {
        let parameters = SANDBOX_MOUNT_ATTACHMENT_TOOL.parameters();
        let properties = parameters
            .get("properties")
            .and_then(Value::as_object)
            .expect("sandbox_mount_attachment schema should have properties");
        assert!(properties.contains_key("attachment_id"));
        assert_eq!(parameters.get("required"), Some(&json!(["attachment_id"])));
    }

    #[test]
    fn oss_download_errors_never_expose_signed_query_values() {
        let url = "https://agentcore-ws-test.oss-cn-hangzhou.aliyuncs.com/quire/attachments/sess/att/data.csv?x-oss-credential=LTAI_TEST%2F20261120%2Fcn-hangzhou%2Foss%2Faliyun_v4_request&x-oss-date=20261120T080000Z&x-oss-expires=600&x-oss-signature-version=OSS4-HMAC-SHA256&x-oss-signature=deadbeef";
        let stderr = format!(
            "curl: (22) The requested URL returned error: 403\n{url}\nx-oss-signature=deadbeef"
        );
        let sanitized = sanitize_oss_download_error(&stderr, url);
        assert!(sanitized.contains("[REDACTED_OSS_URL]"));
        assert!(!sanitized.contains("x-oss-signature="));
        assert!(!sanitized.contains("x-oss-credential="));
        assert!(!sanitized.contains("deadbeef"));
        assert!(!sanitized.contains("LTAI_TEST"));
    }

    #[test]
    fn shell_quote_handles_quotes_and_shell_metacharacters() {
        assert_eq!(shell_quote("data.csv"), "'data.csv'");
        assert_eq!(shell_quote("a'b&c"), "'a'\\''b&c'");
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
}
