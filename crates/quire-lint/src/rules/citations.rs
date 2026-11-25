//! Citation and source-quality checks.

use std::collections::{BTreeSet, HashMap};

use quire_protocol::{BlockKind, CitationScan, ReportDocument, SourceId, safe_href};

use super::evidence::has_quantitative_evidence;
use crate::{
    context::LintContext,
    engine::{Engine, Finding},
    registry::RuleId,
};

pub(crate) fn check(context: &LintContext<'_>) -> Vec<Finding> {
    let mut engine = Engine::new();
    let document = context.document;
    let scan = &context.citation_scan;
    check_citations(document, &mut engine, scan);
    check_malformed_citations(document, &mut engine);
    check_numeric_claims(document, &mut engine, scan);
    check_sources(document, &mut engine);
    engine.into_findings()
}

struct MalformedCitation {
    span: String,
    reason: &'static str,
    hint: String,
}

fn check_malformed_citations(document: &ReportDocument, engine: &mut Engine) {
    let registered: BTreeSet<&str> = document.sources.keys().map(SourceId::as_str).collect();
    let valid_values: Vec<String> = registered.iter().map(|id| (*id).to_string()).collect();
    for (index, block) in document.blocks.iter().enumerate() {
        let mut fields: Vec<(&str, String)> = Vec::new();
        match &block.body {
            BlockKind::Paragraph { text } | BlockKind::Quote { text } => {
                fields.push((text, format!("blocks[{index}].text")));
            }
            BlockKind::Table(table) => {
                if let Some(note) = table.note.as_deref() {
                    fields.push((note, format!("blocks[{index}].note")));
                }
            }
            BlockKind::Echarts(chart) => {
                if let Some(note) = chart.note.as_deref() {
                    fields.push((note, format!("blocks[{index}].note")));
                }
            }
            BlockKind::CustomHtml(html) => {
                if let Some(note) = html.note.as_deref() {
                    fields.push((note, format!("blocks[{index}].note")));
                }
            }
            BlockKind::Heading { .. } => {}
        }
        if let Some(source_note) = block.source_note.as_deref() {
            fields.push((source_note, format!("blocks[{index}].source_note")));
        }
        for (text, path) in fields {
            for malformed in malformed_citations(text, &registered) {
                engine.report_values(
                    RuleId::MalformedCitationSyntax,
                    format!(
                        "block `{}` contains malformed citation syntax `{}`: {}",
                        block.id, malformed.span, malformed.reason
                    ),
                    malformed.hint,
                    Some(block.id.clone()),
                    Some(path.clone()),
                    valid_values.clone(),
                );
            }
        }
    }
}

fn malformed_citations(text: &str, registered: &BTreeSet<&str>) -> Vec<MalformedCitation> {
    let mut findings = Vec::new();
    let mut index = 0usize;
    while index < text.len() {
        let Some(tail) = text.get(index..) else {
            break;
        };
        let Some(ch) = tail.chars().next() else {
            break;
        };
        match ch {
            '\\' => {
                index += ch.len_utf8();
                if let Some(next) = text.get(index..).and_then(|rest| rest.chars().next()) {
                    index += next.len_utf8();
                }
            }
            '`' => {
                index = skip_code_span(text, index);
            }
            '[' => {
                let Some(end) = find_matching_bracket(text, index) else {
                    index += ch.len_utf8();
                    continue;
                };
                let Some(content) = text.get(index + 1..end) else {
                    index = end + 1;
                    continue;
                };
                let after = text.get(end + 1..).unwrap_or("");
                let is_citation_marker = content.starts_with('^');
                if after.starts_with('(') || (after.starts_with('[') && !is_citation_marker) {
                    index = end + 1;
                    continue;
                }
                if let Some((reason, hint)) = malformed_citation(content, registered) {
                    findings.push(MalformedCitation {
                        span: text.get(index..end + 1).unwrap_or(content).to_string(),
                        reason,
                        hint,
                    });
                }
                index = end + 1;
            }
            _ => index += ch.len_utf8(),
        }
    }
    findings
}

fn malformed_citation(
    content: &str,
    registered: &BTreeSet<&str>,
) -> Option<(&'static str, String)> {
    let Some(raw) = content.strip_prefix('^') else {
        let trimmed = content.trim();
        if !trimmed.is_empty() && SourceId::parse(trimmed).is_ok() && registered.contains(trimmed) {
            return Some((
                "missing the `^` citation prefix",
                format!("replace `[{trimmed}]` with `[^{trimmed}]`"),
            ));
        }
        return None;
    };
    if SourceId::parse(raw).is_ok() {
        return None;
    }
    if raw.contains(':') || raw.contains('：') {
        return Some((
            "colon or explanatory text inside the marker",
            single_citation_hint(raw, registered),
        ));
    }
    if has_citation_separator(raw) {
        let ids = split_citation_ids(raw, registered);
        if ids.len() >= 2 {
            return Some((
                "multiple source IDs in one marker",
                consecutive_citations_hint(&ids),
            ));
        }
        return Some((
            "malformed separator or explanatory text inside the marker",
            single_citation_hint(raw, registered),
        ));
    }
    if normalized_id_token(raw).is_some_and(|id| registered.contains(id.as_str())) {
        return Some((
            "whitespace or illegal characters inside the marker",
            single_citation_hint(raw, registered),
        ));
    }
    if let Some(ids) = whitespace_separated_ids(raw, registered) {
        return Some((
            "multiple source IDs in one marker",
            consecutive_citations_hint(&ids),
        ));
    }
    Some((
        "whitespace or illegal characters inside the marker",
        single_citation_hint(raw, registered),
    ))
}

fn has_citation_separator(raw: &str) -> bool {
    raw.chars()
        .any(|ch| matches!(ch, ',' | ';' | '、' | '，' | '；'))
}

fn split_citation_ids(raw: &str, registered: &BTreeSet<&str>) -> Vec<String> {
    let mut ids = Vec::new();
    for part in raw.split([',', ';', '、', '，', '；']) {
        if let Some(id) = best_id_candidate(part, registered)
            && !ids.iter().any(|existing| existing == &id)
        {
            ids.push(id);
        }
    }
    ids
}

fn whitespace_separated_ids(raw: &str, registered: &BTreeSet<&str>) -> Option<Vec<String>> {
    let parts: Vec<&str> = raw.split_whitespace().collect();
    if parts.len() < 2 {
        return None;
    }
    let mut ids = Vec::new();
    for part in parts {
        if SourceId::parse(part).is_err() || !registered.contains(part) {
            return None;
        }
        if !ids
            .iter()
            .any(|existing: &String| existing.as_str() == part)
        {
            ids.push(part.to_string());
        }
    }
    (ids.len() >= 2).then_some(ids)
}

fn consecutive_citations_hint(ids: &[String]) -> String {
    let replacement = ids.iter().map(|id| format!("[^{id}]")).collect::<String>();
    format!("split the merged marker into consecutive citations: `{replacement}`")
}

fn single_citation_hint(raw: &str, registered: &BTreeSet<&str>) -> String {
    if let Some(id) = best_id_candidate(raw, registered) {
        return format!("replace the malformed marker with `[^{id}]`");
    }
    let example = registered
        .iter()
        .next()
        .map(|id| format!("[^{id}]"))
        .unwrap_or_else(|| "[^source_id]".to_string());
    format!("replace the malformed marker with a single pure citation such as `{example}`")
}

fn best_id_candidate(raw: &str, registered: &BTreeSet<&str>) -> Option<String> {
    let trimmed = raw.trim();
    if trimmed.is_empty() {
        return None;
    }
    if SourceId::parse(trimmed).is_ok() && registered.contains(trimmed) {
        return Some(trimmed.to_string());
    }
    let leading = leading_id_token(trimmed);
    if let Some(token) = leading
        && registered.contains(token)
    {
        return Some(token.to_string());
    }
    let normalized = normalized_id_token(trimmed);
    if let Some(id) = normalized.as_deref()
        && registered.contains(id)
    {
        return Some(id.to_string());
    }
    if SourceId::parse(trimmed).is_ok() {
        return Some(trimmed.to_string());
    }
    if let Some(token) = leading {
        return Some(token.to_string());
    }
    normalized
}

fn leading_id_token(raw: &str) -> Option<&str> {
    let end = raw
        .char_indices()
        .find(|(_, ch)| !(ch.is_ascii_alphanumeric() || matches!(ch, '-' | '_' | '.')))
        .map_or(raw.len(), |(index, _)| index);
    let token = raw.get(..end)?;
    if token.is_empty() || SourceId::parse(token).is_err() {
        None
    } else {
        Some(token)
    }
}

fn normalized_id_token(raw: &str) -> Option<String> {
    let trimmed = raw.trim();
    let hard_end = trimmed
        .char_indices()
        .find(|(_, ch)| matches!(ch, ':' | '：' | ',' | '，' | ';' | '；' | '、'))
        .map_or(trimmed.len(), |(index, _)| index);
    let segment = trimmed.get(..hard_end)?;
    let mut normalized = String::new();
    let mut pending_space = false;
    for ch in segment.chars() {
        if ch.is_whitespace() {
            pending_space = true;
        } else if ch.is_ascii_alphanumeric() || matches!(ch, '-' | '_' | '.') {
            if pending_space && !normalized.is_empty() {
                normalized.push('_');
            }
            pending_space = false;
            normalized.push(ch);
        } else {
            break;
        }
    }
    if normalized.is_empty() || SourceId::parse(&normalized).is_err() {
        None
    } else {
        Some(normalized)
    }
}

fn skip_code_span(text: &str, start: usize) -> usize {
    let Some(tail) = text.get(start..) else {
        return start;
    };
    let run = tail.bytes().take_while(|byte| *byte == b'`').count();
    let content_start = start + run;
    let mut search = content_start;
    while let Some(rest) = text.get(search..) {
        let Some(found) = rest.find('`') else {
            break;
        };
        let absolute = search + found;
        let Some(run_tail) = text.get(absolute..) else {
            break;
        };
        let closing_run = run_tail.bytes().take_while(|byte| *byte == b'`').count();
        if closing_run == run {
            return absolute + closing_run;
        }
        search = absolute + closing_run;
    }
    content_start
}

fn find_matching_bracket(text: &str, start: usize) -> Option<usize> {
    let mut depth = 0usize;
    let mut index = start;
    while let Some(tail) = text.get(index..) {
        let ch = tail.chars().next()?;
        if ch == '\\' {
            index += ch.len_utf8();
            if let Some(next) = text.get(index..).and_then(|rest| rest.chars().next()) {
                index += next.len_utf8();
            }
            continue;
        }
        match ch {
            '[' => depth += 1,
            ']' => {
                depth = depth.checked_sub(1)?;
                if depth == 0 {
                    return Some(index);
                }
            }
            _ => {}
        }
        index += ch.len_utf8();
    }
    None
}

fn check_citations(document: &ReportDocument, engine: &mut Engine, scan: &CitationScan) {
    let valid_ids = source_ids(document);
    let mut seen = BTreeSet::new();
    for usage in &scan.dangling {
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
            "register it with upsert_sources, or change the marker to a valid source_id",
            Some(usage.block_id.clone()),
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
        engine.report(
            RuleId::UnusedSource,
            format!("source `{source_id}` is registered but never cited"),
            "add [^<source_id>] to prose or include it in a block source_ids array",
            None,
            Some(format!("sources.{source_id}")),
        );
    }
}

fn check_numeric_claims(document: &ReportDocument, engine: &mut Engine, scan: &CitationScan) {
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
        if !has_quantitative_evidence(text)
            || !block.source_ids.is_empty()
            || block
                .source_note
                .as_deref()
                .is_some_and(|note| !note.trim().is_empty())
            || cited_blocks.contains(block.id.as_str())
        {
            continue;
        }
        engine.report(
            RuleId::NumericClaimWithoutCitation,
            format!(
                "block `{}` states a numeric claim without an inline citation, block source IDs or a source note",
                block.id
            ),
            "In this Paragraph via apply_operations, attach a real `[^source_id]` or `source_ids` entry already retrieved by tools, or name the actual institution/database in `source_note`; never attach an unrelated source merely to clear this diagnostic.",
            Some(block.id.clone()),
            Some(format!("blocks[{index}].text")),
        );
    }
}

fn check_sources(document: &ReportDocument, engine: &mut Engine) {
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
        if let Some(host) = url_host(url)
            && low_authority_domain(&host).is_some()
        {
            engine.report(
                RuleId::LowAuthoritySourceDomain,
                format!(
                    "source `{source_id}` uses low-authority aggregator/wiki domain `{host}`"
                ),
                "In `upsert_sources`, replace wiki or self-media aggregator URLs with primary `.gov.cn`, `.edu.cn`, official statistical communiqué, or authoritative state media sources; add `QR108` to `document.lint.ignore` only if the aggregator itself is the subject.",
                None,
                Some(format!("sources.{source_id}.url")),
            );
        }
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

const LOW_AUTHORITY_DOMAINS: &[&str] = &[
    "baike.baidu.com",
    "baijiahao.baidu.com",
    "toutiao.com",
    "sohu.com",
    "163.com",
    "zhihu.com",
    "csdn.net",
    "jianshu.com",
    "360doc.com",
    "bilibili.com",
    "weibo.com",
    "xiaohongshu.com",
];

fn low_authority_domain(host: &str) -> Option<&'static str> {
    LOW_AUTHORITY_DOMAINS.iter().copied().find(|domain| {
        host == *domain
            || host
                .strip_suffix(domain)
                .is_some_and(|prefix| prefix.ends_with('.'))
    })
}

fn url_host(url: &str) -> Option<String> {
    let (_, rest) = url.trim().split_once("://")?;
    let authority = rest.split(['/', '?', '#']).next().unwrap_or(rest);
    let authority = authority
        .rsplit_once('@')
        .map_or(authority, |(_, host)| host);
    let host = if let Some(rest) = authority.strip_prefix('[') {
        rest.split_once(']').map(|(host, _)| host)?
    } else {
        authority.split(':').next().unwrap_or(authority)
    };
    let host = host.trim_end_matches('.').to_ascii_lowercase();
    let host = host.strip_prefix("www.").unwrap_or(host.as_str());
    (!host.is_empty()).then(|| host.to_string())
}
