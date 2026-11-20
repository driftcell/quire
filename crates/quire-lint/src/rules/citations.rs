//! Citation and source-quality checks.

use std::collections::{BTreeSet, HashMap};

use quire_protocol::{BlockKind, ReportDocument, safe_href, scan_document_citations};

use crate::{engine::Engine, registry::RuleId};

pub(crate) fn check(document: &ReportDocument, engine: &mut Engine<'_>) {
    check_citations(document, engine);
    check_numeric_claims(document, engine);
    check_sources(document, engine);
}

fn check_citations(document: &ReportDocument, engine: &mut Engine<'_>) {
    let scan = scan_document_citations(document);
    let valid_ids = source_ids(document);
    let mut seen = BTreeSet::new();
    for usage in scan.dangling {
        let key = (
            usage.block_id.clone(),
            usage.source_id.clone(),
            usage.location,
        );
        if !seen.insert(key) {
            continue;
        }
        let location = match usage.location {
            quire_protocol::CitationLocation::Inline => "inline citation",
            quire_protocol::CitationLocation::Block => "block-level source_ids",
        };
        engine.report_values(
            RuleId::DanglingSourceId,
            format!(
                "{} `{}` in block `{}` is not registered. Valid source_ids: {}",
                location, usage.source_id, usage.block_id, valid_ids
            ),
            format!(
                "register it with upsert_sources, or change the marker to one of: {}",
                valid_ids
            ),
            Some(usage.block_id),
            Some(format!("sources.{}", usage.source_id)),
            document.sources.keys().map(ToString::to_string).collect(),
        );
    }

    let cited: BTreeSet<_> = scan
        .uses
        .iter()
        .map(|usage| usage.source_id.clone())
        .collect();
    for source_id in document.sources.keys() {
        if cited.contains(source_id) {
            continue;
        }
        engine.report_values(
            RuleId::UnusedSource,
            format!("source `{source_id}` is registered but never cited"),
            format!(
                "add [^{source_id}] to prose or include `{source_id}` in a block source_ids array; valid source_ids: {}",
                valid_ids
            ),
            None,
            Some(format!("sources.{source_id}")),
            document
                .sources
                .keys()
                .map(ToString::to_string)
                .collect(),
        );
    }
}

fn check_numeric_claims(document: &ReportDocument, engine: &mut Engine<'_>) {
    let scan = scan_document_citations(document);
    let cited_blocks: BTreeSet<&str> = scan
        .uses
        .iter()
        .map(|usage| usage.block_id.as_str())
        .collect();
    for (index, block) in document.blocks.iter().enumerate() {
        let text = match &block.body {
            BlockKind::Paragraph { text } | BlockKind::Quote { text } => text,
            _ => continue,
        };
        if !has_numeric_claim(text)
            || !block.source_ids.is_empty()
            || cited_blocks.contains(block.id.as_str())
        {
            continue;
        }
        engine.report(
            RuleId::NumericClaimWithoutCitation,
            format!(
                "block `{}` states a numeric claim without an inline or block-level citation",
                block.id
            ),
            "attach the evidence with `[^source_id]` or `source_ids` when the figure supports a factual claim.",
            Some(block.id.clone()),
            Some(format!("blocks[{index}].text")),
        );
    }
}

fn check_sources(document: &ReportDocument, engine: &mut Engine<'_>) {
    let mut seen_urls: HashMap<String, &str> = HashMap::new();
    for (source_id, source) in &document.sources {
        if source.title.trim().is_empty() {
            engine.report(
                RuleId::SourceTitleEmpty,
                format!("source `{source_id}` has an empty title"),
                "set a title so inline citations can expand to a readable marker",
                None,
                Some(format!("sources.{source_id}.title")),
            );
        }
        let Some(url) = source
            .url
            .as_deref()
            .map(str::trim)
            .filter(|url| !url.is_empty())
        else {
            continue;
        };
        if safe_href(url).is_none() {
            engine.report(
                RuleId::SourceUrlUnsafe,
                format!("source `{source_id}` URL is not a safe http(s)/relative URL"),
                "use an http(s), mailto:, tel:, or relative URL",
                None,
                Some(format!("sources.{source_id}.url")),
            );
        }
        let normalized = url.trim_end_matches('/').to_ascii_lowercase();
        if let Some(first) = seen_urls.get(&normalized) {
            engine.report(
                RuleId::DuplicateSourceUrl,
                format!("source `{source_id}` duplicates the URL registered by `{first}`"),
                "merge duplicate sources through upsert_sources so citations share one canonical entry.",
                None,
                Some(format!("sources.{source_id}.url")),
            );
        } else {
            seen_urls.insert(normalized, source_id.as_str());
        }
    }
}

fn source_ids(document: &ReportDocument) -> String {
    if document.sources.is_empty() {
        "(none registered; call upsert_sources first)".to_string()
    } else {
        document
            .sources
            .keys()
            .map(ToString::to_string)
            .collect::<Vec<_>>()
            .join(", ")
    }
}

fn has_numeric_claim(text: &str) -> bool {
    if text.contains('%')
        || ['¥', '$', '€', '£', '￥']
            .iter()
            .any(|symbol| text.contains(*symbol))
        || ["亿元", "万元", "美元", "人民币", "万人", "亿吨", "万亿"]
            .iter()
            .any(|unit| text.contains(unit))
    {
        return true;
    }
    let chars: Vec<char> = text.chars().collect();
    chars.windows(2).any(|pair| {
        pair.first().is_some_and(char::is_ascii_digit)
            && pair
                .get(1)
                .is_some_and(|ch| matches!(ch, '万' | '亿' | '元' | '人' | '年' | '%'))
    })
}
