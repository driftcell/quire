use super::*;

pub(crate) fn protocol_error(error: ProtocolError) -> ErrorData {
    crate::error::protocol_error(error)
}

pub(crate) fn parse_doc_id(raw: &str) -> Result<DocId, ErrorData> {
    DocId::parse(raw).map_err(protocol_error)
}

pub(crate) fn parse_block_id(raw: &str) -> Result<BlockId, ErrorData> {
    BlockId::parse(raw).map_err(protocol_error)
}

pub(crate) fn parse_block_ids(raw: &[String]) -> Result<Vec<BlockId>, ErrorData> {
    raw.iter().map(|value| parse_block_id(value)).collect()
}

pub(crate) fn parse_revision(value: i64) -> Result<Revision, ErrorData> {
    Revision::new(value).map_err(protocol_error)
}

pub(crate) fn generate_doc_id(title: &str) -> Result<DocId, ErrorData> {
    let slug = slugify_title(title);
    let suffix = short_token(8);
    let prefix: String = slug.chars().take(48).collect();
    DocId::parse(format!("{prefix}-{suffix}")).map_err(protocol_error)
}

pub(crate) fn is_cjk_char(ch: char) -> bool {
    matches!(
        ch,
        '\u{3400}'..='\u{4dbf}' | '\u{4e00}'..='\u{9fff}' | '\u{f900}'..='\u{faff}'
    )
}

pub(crate) fn slugify_title(input: &str) -> String {
    let mut output = String::new();
    let mut pending_dash = false;
    for ch in input.chars() {
        if ch.is_ascii_alphanumeric() {
            if pending_dash && !output.is_empty() {
                output.push('-');
            }
            pending_dash = false;
            for lower in ch.to_lowercase() {
                output.push(lower);
            }
        } else {
            pending_dash = true;
        }
        if output.len() >= 80 {
            break;
        }
    }
    let trimmed = output.trim_matches('-').to_string();
    if trimmed.is_empty() {
        "report".to_string()
    } else {
        trimmed
    }
}

pub(crate) fn inline_source_upserts(
    sources: Option<IndexMap<SourceId, SourceInput>>,
) -> Result<Vec<SourceUpsert>, ErrorData> {
    let Some(sources) = sources else {
        return Ok(Vec::new());
    };
    Ok(sources
        .into_iter()
        .map(|(source_id, input)| SourceUpsert {
            source_id,
            source: input.entry,
        })
        .collect())
}

/// Trim, validate and de-duplicate preview IDs while preserving producer order.
pub(crate) fn normalize_preview_ids(raw: Vec<String>) -> Result<Vec<String>, ErrorData> {
    let mut seen = HashSet::new();
    let mut preview_ids = Vec::with_capacity(raw.len());
    for preview_id in raw {
        let preview_id = preview_id.trim();
        if preview_id.is_empty() {
            return Err(invalid_params(
                "preview_ids must not contain empty values",
                "Pass the Runtime tool_call_id of each ephemeral preview to clear.",
                None,
            ));
        }
        if seen.insert(preview_id.to_string()) {
            preview_ids.push(preview_id.to_string());
        }
    }
    Ok(preview_ids)
}

/// Publish an immediate cleanup frame when a no-op commit leaves previews behind.
pub(crate) async fn publish_ephemeral_clear(
    state: &AppState,
    doc_id: &DocId,
    preview_ids: &[String],
) {
    if preview_ids.is_empty() {
        return;
    }
    let payload = EphemeralPayload::Clear {
        event: EphemeralClearEvent::new(doc_id.clone(), preview_ids.to_vec()),
    };
    if let Err(error) = state
        .store
        .bus()
        .publish_ephemeral(doc_id.as_str(), &payload)
        .await
    {
        tracing::warn!(%error, %doc_id, "failed to publish ephemeral clear");
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use quire_protocol::PageChromeSlots;

    #[test]
    fn slugify_title_drops_non_ascii_characters() {
        assert_eq!(slugify_title("Q3 Revenue 报告"), "q3-revenue");
        assert_eq!(slugify_title("你好"), "report");
        assert_eq!(slugify_title("Report_2026"), "report-2026");
        assert_eq!(slugify_title("!!!"), "report");
    }

    #[test]
    fn generated_doc_ids_start_with_ascii_letters() {
        let generated = generate_doc_id("你好");
        assert!(generated.is_ok());
        if let Ok(doc_id) = generated {
            assert!(doc_id.as_str().starts_with("report-"));
        }
    }

    #[test]
    fn page_chrome_merges_only_supplied_fields() {
        let current = PageChromeConfig {
            header: PageChromeSlots {
                left: "{kicker}".into(),
                center: String::new(),
                right: "{title}".into(),
            },
            footer: PageChromeSlots {
                left: "{title}".into(),
                center: String::new(),
                right: "第 {page} 页".into(),
            },
            hide_header_on_first_page: true,
            hide_footer_on_first_page: false,
            heading_page_break: quire_protocol::HeadingPageBreak::H1,
        };
        let request = ConfigurePageChromeParams {
            doc_id: "doc".into(),
            header: None,
            footer: Some(PageChromeSlots {
                left: String::new(),
                center: "— {page} / {total} —".into(),
                right: String::new(),
            }),
            hide_header_on_first_page: Some(false),
            hide_footer_on_first_page: None,
            heading_page_break: None,
            base_revision: None,
        };
        let merged = QuireReportService::merge_page_chrome(Some(&current), &request);
        assert_eq!(merged.header.left, "{kicker}");
        assert_eq!(merged.footer.center, "— {page} / {total} —");
        assert!(!merged.hide_header_on_first_page);
        assert!(!merged.hide_footer_on_first_page);
        assert_eq!(
            merged.heading_page_break,
            quire_protocol::HeadingPageBreak::H1
        );

        let request = ConfigurePageChromeParams {
            heading_page_break: Some(quire_protocol::HeadingPageBreak::H1AndH2),
            ..request
        };
        let merged = QuireReportService::merge_page_chrome(Some(&current), &request);
        assert_eq!(
            merged.heading_page_break,
            quire_protocol::HeadingPageBreak::H1AndH2
        );
    }
}
