//! Controlled report visual preview orchestration over an internal `mcp/playwright` container.
//!
//! Instead of exposing raw browser automation tools to the agent, `quire-runtime` exposes a single
//! `preview_report` tool that navigates to the dedicated `/docs/{doc_id}/preview` route, waits for
//! `data-preview-ready="true"`, collects active CSS & layout overflow diagnostics, and returns an
//! MCP `ImageContent` screenshot alongside structured inspection metadata.

use std::time::{Duration, Instant};

use quire_protocol::{ChartDataTable, ChartRequirements, EchartsBlock};
use rmcp::model::{CallToolResult, ContentBlock};
use schemars::JsonSchema;
use serde::{Deserialize, Serialize, de::DeserializeOwned};
use serde_json::Value;
#[cfg(test)]
use serde_json::json;

use crate::{
    config::Config,
    error::{RuntimeError, RuntimeResult},
    mcp_client::RmcpClient,
};

pub use tools::{
    CommitChartReceipt, PreviewEchartsReceipt, PreviewToolService, external_mcp_error,
};

mod tools;

const READY_TIMEOUT: Duration = Duration::from_secs(15);
const POLL_INTERVAL: Duration = Duration::from_millis(250);

/// Redis-backed single-chart preview record.
///
/// The chart keeps the original native `option` plus the accumulated incremental `option_patch`,
/// so iterative agent refinements never resend the full ECharts configuration.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ChartPreviewRecord {
    pub preview_id: String,
    pub chart: EchartsBlock,
    #[serde(default)]
    pub requirements: ChartRequirements,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub tables: Vec<ChartDataTable>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub title: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub note: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub based_on_preview_id: Option<String>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub source_ids: Vec<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub source_note: Option<String>,
    /// Authenticated user that created this transient draft. Missing on legacy drafts.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub owner_user_id: Option<String>,
    /// Session that owns this draft. Missing on legacy drafts.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub session_id: Option<String>,
}

#[derive(Debug, Clone, Serialize)]
pub struct ChartPreviewResult {
    pub preview_id: String,
    pub preview_url: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub image_mime_type: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub image_base64: Option<String>,
}

pub use quire_server::report::{PreviewReportInput, PreviewViewMode};

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct OverflowElementDiagnostic {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub block_id: Option<String>,
    pub tag: String,
    pub scroll_width: u64,
    pub client_width: u64,
}

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
pub struct LayoutDiagnostics {
    pub overflow_count: u64,
    pub overflow_elements: Vec<OverflowElementDiagnostic>,
}

#[derive(Debug, Default, Deserialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct DomLayoutInspection {
    #[serde(default)]
    doc_id: Option<String>,
    #[serde(default)]
    revision: Option<u64>,
    #[serde(default)]
    total_pages: Option<usize>,
    #[serde(default)]
    overflow_count: u64,
    #[serde(default)]
    overflow: Vec<OverflowElementDiagnostic>,
    #[serde(default)]
    styles: Option<Value>,
}

#[derive(Debug, Default, Deserialize)]
struct PreviewProbe {
    #[serde(default)]
    ready: bool,
    #[serde(default)]
    error: Option<String>,
    #[serde(default)]
    rendered: bool,
}

#[derive(Serialize)]
struct BrowserNavigateParams<'a> {
    url: &'a str,
}

#[derive(Serialize)]
struct BrowserEvaluateParams<'a> {
    function: &'a str,
}

#[derive(Serialize)]
struct BrowserScreenshotParams {
    #[serde(rename = "fullPage")]
    full_page: bool,
    #[serde(rename = "type")]
    image_type: &'static str,
}

#[derive(Serialize)]
struct PreviewMcpSummary<'a> {
    doc_id: &'a str,
    preview_url: &'a str,
    mode: PreviewViewMode,
    revision: u64,
    total_pages: usize,
    layout_diagnostics: &'a LayoutDiagnostics,
    styles: Option<&'a Value>,
}

#[derive(Serialize)]
struct PreviewImageContent<'a> {
    #[serde(rename = "type")]
    kind: &'static str,
    #[serde(rename = "mimeType")]
    mime_type: &'a str,
    data: &'a str,
}

#[derive(Serialize)]
struct PreviewToolPayload<'a> {
    doc_id: &'a str,
    preview_url: &'a str,
    mode: PreviewViewMode,
    revision: u64,
    total_pages: usize,
    layout_diagnostics: &'a LayoutDiagnostics,
    styles: Option<&'a Value>,
    image_included: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    image_content: Option<PreviewImageContent<'a>>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PreviewReportResult {
    pub doc_id: String,
    pub preview_url: String,
    pub mode: PreviewViewMode,
    pub revision: u64,
    pub total_pages: usize,
    pub layout_diagnostics: LayoutDiagnostics,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub styles: Option<Value>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub image_mime_type: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub image_base64: Option<String>,
}

impl PreviewReportResult {
    /// Build an MCP `CallToolResult` with the PNG `ImageContent` when requested, plus a JSON summary.
    pub fn to_mcp_call_tool_result(&self) -> CallToolResult {
        let summary = PreviewMcpSummary {
            doc_id: &self.doc_id,
            preview_url: &self.preview_url,
            mode: self.mode,
            revision: self.revision,
            total_pages: self.total_pages,
            layout_diagnostics: &self.layout_diagnostics,
            styles: self.styles.as_ref(),
        };
        let mut content = Vec::new();
        if let (Some(data), Some(mime)) = (&self.image_base64, &self.image_mime_type) {
            content.push(ContentBlock::image(data.clone(), mime.clone()));
        }
        content.push(ContentBlock::text(
            serde_json::to_string_pretty(&summary).unwrap_or_default(),
        ));
        CallToolResult::success(content)
    }

    fn tool_payload(&self, include_image_content: bool) -> PreviewToolPayload<'_> {
        let image_content = if include_image_content {
            match (&self.image_base64, &self.image_mime_type) {
                (Some(data), Some(mime)) => Some(PreviewImageContent {
                    kind: "image",
                    mime_type: mime,
                    data,
                }),
                _ => None,
            }
        } else {
            None
        };
        PreviewToolPayload {
            doc_id: &self.doc_id,
            preview_url: &self.preview_url,
            mode: self.mode,
            revision: self.revision,
            total_pages: self.total_pages,
            layout_diagnostics: &self.layout_diagnostics,
            styles: self.styles.as_ref(),
            image_included: self.image_base64.is_some(),
            image_content,
        }
    }

    /// Build a text-only diagnostic JSON payload (never embeds raw base64).
    pub fn to_diagnostics_json(&self) -> RuntimeResult<Value> {
        Ok(serde_json::to_value(self.tool_payload(false))?)
    }

    /// Build a JSON payload for internal `DynamicTool` execution. The screenshot is
    /// omitted unless the caller explicitly requested it.
    pub fn to_tool_json(&self) -> RuntimeResult<Value> {
        Ok(serde_json::to_value(self.tool_payload(true))?)
    }
}

fn percent_encode(input: &str) -> String {
    use std::fmt::Write as _;
    let mut out = String::with_capacity(input.len());
    for byte in input.as_bytes() {
        if byte.is_ascii_alphanumeric() || matches!(byte, b'-' | b'_' | b'.' | b'~') {
            out.push(char::from(*byte));
        } else {
            let _ = write!(out, "%{byte:02X}");
        }
    }
    out
}

pub fn build_preview_url(config: &Config, input: &PreviewReportInput) -> String {
    build_preview_url_for_base(
        &config.preview_base_url,
        crate::auth::issue_doc_preview_jwt(config, input.doc_id.trim())
            .unwrap_or_default()
            .as_str(),
        input,
        true,
    )
}

fn build_public_preview_url(config: &Config, input: &PreviewReportInput) -> String {
    build_preview_url_for_base(&config.preview_public_url, "", input, false)
}

fn build_preview_url_for_base(
    base: &str,
    ro_token: &str,
    input: &PreviewReportInput,
    include_ro_token: bool,
) -> String {
    let base = base.trim_end_matches('/');
    let mut query_parts = vec![format!(
        "mode={}",
        match input.mode {
            PreviewViewMode::Paged => "paged",
            PreviewViewMode::Continuous => "continuous",
        }
    )];
    if let Some(section_id) = input
        .section_id
        .as_deref()
        .map(str::trim)
        .filter(|s| !s.is_empty())
    {
        query_parts.push(format!("sectionId={}", percent_encode(section_id)));
    }
    if let Some(block_ids) = input.block_ids.as_ref().filter(|ids| !ids.is_empty()) {
        let joined = block_ids
            .iter()
            .map(|s| s.trim())
            .filter(|s| !s.is_empty())
            .collect::<Vec<_>>()
            .join(",");
        if !joined.is_empty() {
            query_parts.push(format!("blockIds={}", percent_encode(&joined)));
        }
    }
    if let Some(page) = input.page.filter(|p| *p >= 1) {
        query_parts.push(format!("page={page}"));
    }
    if let Some(rev) = input.rev {
        query_parts.push(format!("rev={rev}"));
    }

    let ro_token = ro_token.trim();
    let hash = if include_ro_token && !ro_token.is_empty() {
        format!("#ro_token={}", percent_encode(ro_token))
    } else {
        String::new()
    };

    format!(
        "{base}/docs/{}/preview?{}{}",
        percent_encode(input.doc_id.trim()),
        query_parts.join("&"),
        hash
    )
}

fn chart_preview_path(preview_id: &str) -> String {
    format!("/charts/{}/preview", percent_encode(preview_id.trim()))
}

/// Internal URL opened by Playwright MCP. The read-only token stays in the hash fragment.
pub fn build_chart_preview_url(config: &Config, preview_id: &str) -> String {
    let base = config.preview_base_url.trim_end_matches('/');
    let ro_token =
        crate::auth::issue_chart_preview_jwt(config, preview_id.trim()).unwrap_or_default();
    let hash = if ro_token.is_empty() {
        String::new()
    } else {
        format!("#ro_token={}", percent_encode(ro_token.trim()))
    };
    format!("{base}{}{hash}", chart_preview_path(preview_id))
}

/// Public URL returned to agents; it never contains the read-only token.
pub fn build_public_chart_preview_url(config: &Config, preview_id: &str) -> String {
    let base = config.preview_public_url.trim_end_matches('/');
    format!("{base}{}", chart_preview_path(preview_id))
}

mod browser;
#[cfg(test)]
mod tests;

#[cfg(test)]
pub(crate) use browser::{checked_preview_identity, extract_eval_json};
pub use browser::{execute_preview_echarts, execute_preview_report};
