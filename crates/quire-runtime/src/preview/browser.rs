use super::*;

pub(crate) fn extract_eval_json<T: DeserializeOwned>(raw: &Value) -> Option<T> {
    if let Some(structured) = raw.get("structuredContent") {
        return serde_json::from_value(structured.clone()).ok();
    }
    let text = raw
        .get("content")
        .and_then(Value::as_array)?
        .iter()
        .find_map(|item| item.get("text").and_then(Value::as_str))?;
    // mcp/playwright wraps evaluation results in markdown/text blocks like `### Result\n...`
    if let Ok(parsed) = serde_json::from_str::<Value>(text) {
        return serde_json::from_value(parsed).ok();
    }
    // mcp/playwright pretty-prints evaluation results under `### Result`, so
    // parse the first JSON value after that heading instead of matching lines.
    if let Some(index) = text.find("### Result")
        && let Some(rest) = text.get(index + "### Result".len()..)
    {
        let mut values = serde_json::Deserializer::from_str(rest.trim_start()).into_iter::<Value>();
        if let Some(Ok(parsed)) = values.next() {
            return serde_json::from_value(parsed).ok();
        }
    }
    for line in text.lines() {
        let trimmed = line.trim();
        if ((trimmed.starts_with('{') && trimmed.ends_with('}'))
            || trimmed == "true"
            || trimmed == "false")
            && let Ok(parsed) = serde_json::from_str::<Value>(trimmed)
        {
            return serde_json::from_value(parsed).ok();
        }
    }
    // Also check fenced JSON blocks
    if let Some(start) = text.find("```json") {
        let rest = &text[start + 7..];
        if let Some(end) = rest.find("```")
            && let Ok(parsed) = serde_json::from_str::<Value>(rest[..end].trim())
        {
            return serde_json::from_value(parsed).ok();
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
        .call_tool_raw(
            "browser_navigate",
            serde_json::to_value(BrowserNavigateParams { url: &preview_url })?,
        )
        .await?;

    let started = Instant::now();
    loop {
        let probe = client
            .call_tool_raw(
                "browser_evaluate",
                serde_json::to_value(BrowserEvaluateParams {
                    function: "() => ({ ready: Boolean(document.querySelector('[data-preview-ready=\"true\"]')), error: document.querySelector('[data-preview-error=\"true\"]')?.textContent ?? null })",
                })?,
            )
            .await?;

        if let Some(val) = extract_eval_json::<PreviewProbe>(&probe) {
            if let Some(err_msg) = val.error {
                return Err(RuntimeError::Internal(format!(
                    "Preview page failed to load: {err_msg}"
                )));
            }
            if val.ready {
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
        .call_tool_raw(
            "browser_evaluate",
            serde_json::to_value(BrowserEvaluateParams {
                function: &inspect_fn,
            })?,
        )
        .await?;
    let inspected: DomLayoutInspection = extract_eval_json(&inspected_raw).ok_or_else(|| {
        RuntimeError::Internal("Preview inspection did not return diagnostics".into())
    })?;
    let (revision, total_pages) = checked_preview_identity(&inspected, &doc_id, input.rev)?;

    let (image_base64, image_mime_type) = if input.include_image {
        let screenshot_raw = client
            .call_tool_raw(
                "browser_take_screenshot",
                serde_json::to_value(BrowserScreenshotParams {
                    full_page: true,
                    image_type: "png",
                })?,
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
        layout_diagnostics: LayoutDiagnostics {
            overflow_count: inspected.overflow_count,
            overflow_elements: inspected.overflow,
        },
        styles: if input.include_styles {
            inspected.styles
        } else {
            None
        },
        image_mime_type,
        image_base64,
    })
}

pub(crate) fn checked_preview_identity(
    inspected: &DomLayoutInspection,
    doc_id: &str,
    expected: Option<u64>,
) -> RuntimeResult<(u64, usize)> {
    let revision = inspected
        .revision
        .filter(|revision| *revision > 0)
        .ok_or_else(|| {
            RuntimeError::Internal("Preview did not identify a committed revision".into())
        })?;
    if inspected.doc_id.as_deref() != Some(doc_id) {
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
    let pages = inspected
        .total_pages
        .filter(|pages| *pages > 0)
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
        .call_tool_raw(
            "browser_navigate",
            serde_json::to_value(BrowserNavigateParams { url: &preview_url })?,
        )
        .await?;

    let started = Instant::now();
    loop {
        let probe = client
            .call_tool_raw(
                "browser_evaluate",
                serde_json::to_value(BrowserEvaluateParams {
                    function: "() => ({ ready: Boolean(document.querySelector('[data-preview-ready=\"true\"]')), error: document.querySelector('[data-preview-error=\"true\"]')?.textContent ?? null, rendered: Boolean(document.querySelector('[data-chart-canvas] canvas, [data-chart-canvas] svg')) })",
                })?,
            )
            .await?;

        if let Some(val) = extract_eval_json::<PreviewProbe>(&probe) {
            if let Some(err_msg) = val.error {
                return Err(RuntimeError::Internal(format!(
                    "Chart preview page failed to load: {err_msg}"
                )));
            }
            if val.ready && val.rendered {
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
            serde_json::to_value(BrowserScreenshotParams {
                full_page: true,
                image_type: "png",
            })?,
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
