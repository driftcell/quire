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
use secrecy::ExposeSecret;
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};

use crate::{
    config::Config,
    error::{RuntimeError, RuntimeResult},
    mcp_client::RmcpClient,
};

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

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum PreviewViewMode {
    #[default]
    Paged,
    Continuous,
}

#[derive(Debug, Clone, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct PreviewReportInput {
    /// Report document ID to preview.
    pub doc_id: String,
    /// Layout mode (`paged` for A4 pagination, `continuous` for single-column scroll).
    #[serde(default)]
    pub mode: PreviewViewMode,
    /// Optional heading block_id to preview only that section.
    #[serde(default)]
    pub section_id: Option<String>,
    /// Optional list of specific block IDs to preview.
    #[serde(default)]
    pub block_ids: Option<Vec<String>>,
    /// Optional 1-based A4 page number when `mode` is `paged`.
    #[serde(default)]
    pub page: Option<usize>,
    /// Optional exact committed revision (>=1); a different revision is never accepted.
    #[serde(default)]
    pub rev: Option<u64>,
    /// Include active theme tokens, custom_css, and computed styles for key selectors.
    #[serde(default = "default_include_styles")]
    pub include_styles: bool,
    /// Include the base64 PNG screenshot. Defaults to true for MCP visual clients;
    /// the text-only agent tool forces this off.
    #[serde(default = "default_include_image")]
    pub include_image: bool,
}

fn default_include_styles() -> bool {
    true
}

fn default_include_image() -> bool {
    true
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PreviewReportResult {
    pub doc_id: String,
    pub preview_url: String,
    pub mode: PreviewViewMode,
    pub revision: u64,
    pub total_pages: usize,
    pub layout_diagnostics: Value,
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
        let summary = json!({
            "doc_id": self.doc_id,
            "preview_url": self.preview_url,
            "mode": self.mode,
            "revision": self.revision,
            "total_pages": self.total_pages,
            "layout_diagnostics": self.layout_diagnostics,
            "styles": self.styles,
        });
        let mut content = Vec::new();
        if let (Some(data), Some(mime)) = (&self.image_base64, &self.image_mime_type) {
            content.push(ContentBlock::image(data.clone(), mime.clone()));
        }
        content.push(ContentBlock::text(
            serde_json::to_string_pretty(&summary).unwrap_or_default(),
        ));
        CallToolResult::success(content)
    }

    /// Build a text-only diagnostic JSON payload (never embeds raw base64).
    pub fn to_diagnostics_json(&self) -> Value {
        json!({
            "doc_id": self.doc_id,
            "preview_url": self.preview_url,
            "mode": self.mode,
            "revision": self.revision,
            "total_pages": self.total_pages,
            "layout_diagnostics": self.layout_diagnostics,
            "styles": self.styles,
            "image_included": self.image_base64.is_some(),
        })
    }

    /// Build a JSON payload for internal `DynamicTool` execution. The screenshot is
    /// omitted unless the caller explicitly requested it.
    pub fn to_tool_json(&self) -> Value {
        let mut payload = self.to_diagnostics_json();
        if let (Some(data), Some(mime)) = (&self.image_base64, &self.image_mime_type)
            && let Some(object) = payload.as_object_mut()
        {
            object.insert(
                "image_content".to_string(),
                json!({
                    "type": "image",
                    "mimeType": mime,
                    "data": data,
                }),
            );
        }
        payload
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
        config.quire_ro_token.expose_secret(),
        input,
        true,
    )
}

fn build_public_preview_url(config: &Config, input: &PreviewReportInput) -> String {
    build_preview_url_for_base(
        &config.preview_public_url,
        config.quire_ro_token.expose_secret(),
        input,
        false,
    )
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
    let ro_token = config.quire_ro_token.expose_secret().trim();
    let hash = if ro_token.is_empty() {
        String::new()
    } else {
        format!("#ro_token={}", percent_encode(ro_token))
    };
    format!("{base}{}{hash}", chart_preview_path(preview_id))
}

/// Public URL returned to agents; it never contains the read-only token.
pub fn build_public_chart_preview_url(config: &Config, preview_id: &str) -> String {
    let base = config.preview_public_url.trim_end_matches('/');
    format!("{base}{}", chart_preview_path(preview_id))
}

fn extract_eval_json(raw: &Value) -> Option<Value> {
    if let Some(structured) = raw.get("structuredContent") {
        return Some(structured.clone());
    }
    let text = raw
        .get("content")
        .and_then(Value::as_array)?
        .iter()
        .find_map(|item| item.get("text").and_then(Value::as_str))?;
    // mcp/playwright wraps evaluation results in markdown/text blocks like `### Result\n...`
    if let Ok(parsed) = serde_json::from_str::<Value>(text) {
        return Some(parsed);
    }
    // mcp/playwright pretty-prints evaluation results under `### Result`, so
    // parse the first JSON value after that heading instead of matching lines.
    if let Some(index) = text.find("### Result")
        && let Some(rest) = text.get(index + "### Result".len()..)
    {
        let mut values = serde_json::Deserializer::from_str(rest.trim_start()).into_iter::<Value>();
        if let Some(Ok(parsed)) = values.next() {
            return Some(parsed);
        }
    }
    for line in text.lines() {
        let trimmed = line.trim();
        if ((trimmed.starts_with('{') && trimmed.ends_with('}'))
            || trimmed == "true"
            || trimmed == "false")
            && let Ok(parsed) = serde_json::from_str::<Value>(trimmed)
        {
            return Some(parsed);
        }
    }
    // Also check fenced JSON blocks
    if let Some(start) = text.find("```json") {
        let rest = &text[start + 7..];
        if let Some(end) = rest.find("```")
            && let Ok(parsed) = serde_json::from_str::<Value>(rest[..end].trim())
        {
            return Some(parsed);
        }
    }
    None
}

fn extract_image_content(raw: &Value) -> Option<(String, String)> {
    let items = raw.get("content").and_then(Value::as_array)?;
    for item in items {
        if item.get("type").and_then(Value::as_str) == Some("image") {
            let data = item.get("data").and_then(Value::as_str)?;
            let mime = item
                .get("mimeType")
                .and_then(Value::as_str)
                .unwrap_or("image/png");
            return Some((data.to_string(), mime.to_string()));
        }
    }
    None
}

/// Navigate to the preview route via `mcp/playwright`, wait for readiness, inspect CSS/layout,
/// and capture a PNG screenshot.
pub async fn execute_preview_report(
    config: &Config,
    input: PreviewReportInput,
) -> RuntimeResult<PreviewReportResult> {
    let doc_id = input.doc_id.trim().to_string();
    if doc_id.is_empty() {
        return Err(RuntimeError::InvalidRequest("`doc_id` is required".into()));
    }
    if input.rev == Some(0) {
        return Err(RuntimeError::InvalidRequest(
            "rev must be a committed revision (>=1)".into(),
        ));
    }
    let playwright_url = config
        .playwright_mcp_url
        .as_deref()
        .map(str::trim)
        .filter(|s| !s.is_empty())
        .ok_or_else(|| {
            RuntimeError::InvalidRequest(
                "PLAYWRIGHT_MCP_URL is not configured on quire-runtime".into(),
            )
        })?;

    let preview_url = build_preview_url(config, &input);
    let public_preview_url = build_public_preview_url(config, &input);
    let client = RmcpClient::connect(playwright_url, None, "preview", "quire-preview").await?;

    client
        .call_tool_raw("browser_navigate", json!({ "url": preview_url }))
        .await?;

    let started = Instant::now();
    loop {
        let probe = client
            .call_tool_raw(
                "browser_evaluate",
                json!({
                    "function": "() => ({ ready: Boolean(document.querySelector('[data-preview-ready=\"true\"]')), error: document.querySelector('[data-preview-error=\"true\"]')?.textContent ?? null })"
                }),
            )
            .await?;

        if let Some(val) = extract_eval_json(&probe) {
            if let Some(err_msg) = val.get("error").and_then(Value::as_str) {
                return Err(RuntimeError::Internal(format!(
                    "Preview page failed to load: {err_msg}"
                )));
            }
            if val.get("ready").and_then(Value::as_bool) == Some(true) {
                break;
            }
        }
        if started.elapsed() >= READY_TIMEOUT {
            return Err(RuntimeError::Internal(
                "Timed out waiting for preview page [data-preview-ready=\"true\"]".into(),
            ));
        }
        tokio::time::sleep(POLL_INTERVAL).await;
    }

    let inspect_fn = format!(
        r#"() => {{
  const root = document.querySelector('[data-preview-root="true"]');
  const reportRoot = document.querySelector('.report-root');
  const overflow = [];
  if (reportRoot) {{
    for (const el of reportRoot.querySelectorAll('[data-block-id], table, figure')) {{
      if (el.scrollWidth > el.clientWidth + 2) {{
        overflow.push({{
          tag: el.tagName.toLowerCase(),
          blockId: el.getAttribute('data-block-id') || el.id || null,
          scrollWidth: el.scrollWidth,
          clientWidth: el.clientWidth,
        }});
      }}
    }}
  }}
  const includeStyles = {};
  let styles = null;
  if (includeStyles) {{
    const pick = (sel) => {{
      const el = document.querySelector(sel);
      if (!el) return null;
      const cs = getComputedStyle(el);
      return {{
        fontFamily: cs.fontFamily,
        fontSize: cs.fontSize,
        lineHeight: cs.lineHeight,
        color: cs.color,
        backgroundColor: cs.backgroundColor,
        padding: cs.padding,
      }};
    }};
    styles = {{
      meta: window.__QUIRE_PREVIEW_META__ || null,
      computed: {{
        reportRoot: pick('.report-root'),
        h1: pick('.report-root h1'),
        h2: pick('.report-root h2'),
        paragraph: pick('.report-root p'),
        table: pick('.report-root table'),
      }},
    }};
  }}
  return {{
    docId: root?.getAttribute('data-doc-id'),
    revision: Number(root?.getAttribute('data-revision')),
    totalPages: Number(root?.getAttribute('data-total-pages')),
    overflowCount: overflow.length,
    overflow: overflow.slice(0, 10),
    styles,
  }};
}}"#,
        if input.include_styles {
            "true"
        } else {
            "false"
        }
    );

    let inspected_raw = client
        .call_tool_raw("browser_evaluate", json!({ "function": inspect_fn }))
        .await?;
    let inspected = extract_eval_json(&inspected_raw).ok_or_else(|| {
        RuntimeError::Internal("Preview inspection did not return diagnostics".into())
    })?;
    let (revision, total_pages) = checked_preview_identity(&inspected, &doc_id, input.rev)?;

    let (image_base64, image_mime_type) = if input.include_image {
        let screenshot_raw = client
            .call_tool_raw(
                "browser_take_screenshot",
                json!({ "fullPage": true, "type": "png" }),
            )
            .await?;
        let (data, mime) = extract_image_content(&screenshot_raw).ok_or_else(|| {
            RuntimeError::Internal(
                "Playwright MCP browser_take_screenshot did not return ImageContent".into(),
            )
        })?;
        (Some(data), Some(mime))
    } else {
        (None, None)
    };

    Ok(PreviewReportResult {
        doc_id,
        preview_url: public_preview_url,
        mode: input.mode,
        revision,
        total_pages,
        layout_diagnostics: json!({
            "overflow_count": inspected.get("overflowCount").and_then(Value::as_u64).unwrap_or(0),
            "overflow_elements": inspected.get("overflow").cloned().unwrap_or_else(|| json!([])),
        }),
        styles: if input.include_styles {
            inspected.get("styles").cloned()
        } else {
            None
        },
        image_mime_type,
        image_base64,
    })
}

fn checked_preview_identity(
    inspected: &Value,
    doc_id: &str,
    expected: Option<u64>,
) -> RuntimeResult<(u64, usize)> {
    let revision = inspected["revision"]
        .as_u64()
        .filter(|r| *r > 0)
        .ok_or_else(|| {
            RuntimeError::Internal("Preview did not identify a committed revision".into())
        })?;
    if inspected["docId"].as_str() != Some(doc_id) {
        return Err(RuntimeError::Internal(format!(
            "Preview did not render document `{doc_id}`"
        )));
    }
    if let Some(expected) = expected
        && revision != expected
    {
        return Err(RuntimeError::InvalidRequest(format!(
            "Expected preview revision {expected}, rendered {revision}"
        )));
    }
    let pages = inspected["totalPages"]
        .as_u64()
        .and_then(|v| usize::try_from(v).ok())
        .filter(|n| *n > 0)
        .ok_or_else(|| RuntimeError::Internal("Preview did not identify its page count".into()))?;
    Ok((revision, pages))
}

/// Navigate to the stateless single-chart preview route, wait for the chart to render, and capture
/// a PNG screenshot via `mcp/playwright`.
pub async fn execute_preview_echarts(
    config: &Config,
    preview_id: &str,
) -> RuntimeResult<ChartPreviewResult> {
    let preview_id = preview_id.trim();
    if preview_id.is_empty() {
        return Err(RuntimeError::InvalidRequest(
            "`preview_id` is required".into(),
        ));
    }
    let playwright_url = config
        .playwright_mcp_url
        .as_deref()
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .ok_or_else(|| {
            RuntimeError::InvalidRequest(
                "PLAYWRIGHT_MCP_URL is not configured on quire-runtime".into(),
            )
        })?;

    let preview_url = build_chart_preview_url(config, preview_id);
    let public_preview_url = build_public_chart_preview_url(config, preview_id);
    let client = RmcpClient::connect(playwright_url, None, "preview", "quire-preview").await?;

    client
        .call_tool_raw("browser_navigate", json!({ "url": preview_url }))
        .await?;

    let started = Instant::now();
    loop {
        let probe = client
            .call_tool_raw(
                "browser_evaluate",
                json!({
                    "function": "() => ({ ready: Boolean(document.querySelector('[data-preview-ready=\"true\"]')), error: document.querySelector('[data-preview-error=\"true\"]')?.textContent ?? null, rendered: Boolean(document.querySelector('[data-chart-canvas] canvas, [data-chart-canvas] svg')) })"
                }),
            )
            .await?;

        if let Some(val) = extract_eval_json(&probe) {
            if let Some(err_msg) = val.get("error").and_then(Value::as_str) {
                return Err(RuntimeError::Internal(format!(
                    "Chart preview page failed to load: {err_msg}"
                )));
            }
            if val.get("ready").and_then(Value::as_bool) == Some(true)
                && val.get("rendered").and_then(Value::as_bool) == Some(true)
            {
                break;
            }
        }
        if started.elapsed() >= READY_TIMEOUT {
            return Err(RuntimeError::Internal(
                "Timed out waiting for the chart preview to render".into(),
            ));
        }
        tokio::time::sleep(POLL_INTERVAL).await;
    }

    let screenshot_raw = client
        .call_tool_raw(
            "browser_take_screenshot",
            json!({ "fullPage": true, "type": "png" }),
        )
        .await?;
    let (data, mime) = extract_image_content(&screenshot_raw).ok_or_else(|| {
        RuntimeError::Internal(
            "Playwright MCP browser_take_screenshot did not return ImageContent".into(),
        )
    })?;

    Ok(ChartPreviewResult {
        preview_id: preview_id.to_string(),
        preview_url: public_preview_url,
        image_mime_type: Some(mime),
        image_base64: Some(data),
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_pretty_printed_playwright_eval_results() {
        let raw = json!({
            "content": [{
                "type": "text",
                "text": "### Result\n{\n  \"ready\": true,\n  \"error\": null\n}\n### Ran Playwright code\n```js\nawait page.evaluate('() => ({})');\n```"
            }]
        });
        let parsed = extract_eval_json(&raw);
        assert_eq!(
            parsed
                .as_ref()
                .and_then(|value| value.get("ready"))
                .and_then(Value::as_bool),
            Some(true)
        );
        assert_eq!(
            parsed.as_ref().and_then(|value| value.get("error")),
            Some(&Value::Null)
        );
    }

    fn preview_result(image: Option<(&str, &str)>) -> PreviewReportResult {
        PreviewReportResult {
            doc_id: "report-test".into(),
            preview_url: "http://localhost:5173/docs/report-test/preview".into(),
            mode: PreviewViewMode::Paged,
            revision: 3,
            total_pages: 2,
            layout_diagnostics: json!({"overflow_count": 0, "overflow_elements": []}),
            styles: None,
            image_mime_type: image.map(|(_, mime)| mime.to_string()),
            image_base64: image.map(|(data, _)| data.to_string()),
        }
    }

    #[test]
    fn preview_identity_rejects_a_different_revision() {
        let inspected = json!({
            "docId": "report-test",
            "revision": 11,
            "totalPages": 2
        });
        let error = checked_preview_identity(&inspected, "report-test", Some(10)).unwrap_err();
        assert!(
            error
                .to_string()
                .contains("Expected preview revision 10, rendered 11")
        );
    }

    #[test]
    fn preview_identity_rejects_a_different_document() {
        let inspected = json!({
            "docId": "another-report",
            "revision": 10,
            "totalPages": 2
        });
        let error = checked_preview_identity(&inspected, "report-test", None).unwrap_err();
        assert!(error.to_string().contains("report-test"));
    }

    #[test]
    fn public_preview_url_omits_the_ro_token() {
        let input = PreviewReportInput {
            doc_id: "report-test".into(),
            mode: PreviewViewMode::Paged,
            section_id: None,
            block_ids: None,
            page: None,
            rev: None,
            include_styles: false,
            include_image: false,
        };
        let public = build_preview_url_for_base("http://localhost:5173", "secret", &input, false);
        assert!(!public.contains("ro_token"));
        let internal = build_preview_url_for_base("http://localhost:5173", "secret", &input, true);
        assert!(internal.contains("ro_token=secret"));
    }

    #[test]
    fn chart_preview_urls_keep_the_token_in_the_hash_only() {
        let mut config = Config::from_env();
        config.preview_base_url = "http://127.0.0.1:5173".into();
        config.preview_public_url = "http://localhost:5173".into();
        config.quire_ro_token = secrecy::SecretString::from("secret".to_string());
        let internal = build_chart_preview_url(&config, "chart_abc");
        assert_eq!(
            internal,
            "http://127.0.0.1:5173/charts/chart_abc/preview#ro_token=secret"
        );
        let public = build_public_chart_preview_url(&config, "chart_abc");
        assert_eq!(public, "http://localhost:5173/charts/chart_abc/preview");
        assert!(!public.contains("secret"));
    }

    #[test]
    fn tool_json_omits_the_image_when_not_requested() {
        let payload = preview_result(None).to_tool_json();
        assert_eq!(
            payload.get("image_included").and_then(Value::as_bool),
            Some(false)
        );
        assert!(payload.get("image_content").is_none());
        assert_eq!(
            payload.get("preview_url").and_then(Value::as_str),
            Some("http://localhost:5173/docs/report-test/preview")
        );
    }

    #[test]
    fn tool_json_includes_the_image_when_requested() {
        let payload = preview_result(Some(("cG5n", "image/png"))).to_tool_json();
        assert_eq!(
            payload.get("image_included").and_then(Value::as_bool),
            Some(true)
        );
        assert_eq!(
            payload
                .get("image_content")
                .and_then(|value| value.get("mimeType"))
                .and_then(Value::as_str),
            Some("image/png")
        );
    }
}
