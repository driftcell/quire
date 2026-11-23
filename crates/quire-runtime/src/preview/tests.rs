use super::*;

#[test]
fn parses_pretty_printed_playwright_eval_results() {
    let raw = json!({
        "content": [{
            "type": "text",
            "text": "### Result\n{\n  \"ready\": true,\n  \"error\": null\n}\n### Ran Playwright code\n```js\nawait page.evaluate('() => ({})');\n```"
        }]
    });
    let parsed = extract_eval_json::<Value>(&raw);
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
        layout_diagnostics: LayoutDiagnostics::default(),
        styles: None,
        image_mime_type: image.map(|(_, mime)| mime.to_string()),
        image_base64: image.map(|(data, _)| data.to_string()),
    }
}

#[test]
fn preview_identity_rejects_a_different_revision() {
    let inspected = DomLayoutInspection {
        doc_id: Some("report-test".into()),
        revision: Some(11),
        total_pages: Some(2),
        ..DomLayoutInspection::default()
    };
    let error = checked_preview_identity(&inspected, "report-test", Some(10)).unwrap_err();
    assert!(
        error
            .to_string()
            .contains("Expected preview revision 10, rendered 11")
    );
}

#[test]
fn preview_identity_rejects_a_different_document() {
    let inspected = DomLayoutInspection {
        doc_id: Some("another-report".into()),
        revision: Some(10),
        total_pages: Some(2),
        ..DomLayoutInspection::default()
    };
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
    let payload = preview_result(None).to_tool_json().unwrap();
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
    let payload = preview_result(Some(("cG5n", "image/png")))
        .to_tool_json()
        .unwrap();
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
