//! Canonical Markdown rendering for report tool receipts shared across native Rig tools and rmcp.

use askama::Template;
use quire_protocol::{CommentStatus, Diagnostic, Severity};
use rmcp::ErrorData;

use crate::{guide::GuideTopic, report::schema as contracts};

pub(super) fn render_template<T: Template>(template: &T) -> Result<String, ErrorData> {
    template
        .render()
        .map_err(|error| ErrorData::internal_error(error.to_string(), None))
}

#[derive(Template)]
#[template(path = "tools/read_as_markdown.md", escape = "none")]
struct ReadAsMarkdownTemplate<'a> {
    doc_id: &'a str,
    revision: i64,
    markdown_chars: usize,
    markdown: &'a str,
    next_hint: &'a str,
}

impl contracts::ReadMarkdownReceipt {
    pub fn render_markdown(&self) -> Result<String, ErrorData> {
        render_template(&ReadAsMarkdownTemplate {
            doc_id: &self.doc_id,
            revision: self.revision,
            markdown_chars: self.markdown_chars,
            markdown: &self.markdown,
            next_hint: &self._next_hint,
        })
    }
}

pub fn render_read_as_markdown(
    receipt: &contracts::ReadMarkdownReceipt,
) -> Result<String, ErrorData> {
    receipt.render_markdown()
}

#[derive(Template)]
#[template(path = "tools/get_protocol_guide.md", escape = "none")]
struct GetProtocolGuideTemplate<'a> {
    topic: &'static str,
    guide: &'a str,
    recommended_tools: String,
    next_hint: &'a str,
}

impl contracts::GuideReceipt {
    pub fn render_markdown(&self) -> Result<String, ErrorData> {
        let topic = match self.topic {
            GuideTopic::Quickstart => "quickstart",
            GuideTopic::TablesAndCharts => "tables_and_charts",
            GuideTopic::CitationsAndCss => "citations_and_css",
            GuideTopic::ParallelWorkflow => "parallel_workflow",
        };
        render_template(&GetProtocolGuideTemplate {
            topic,
            guide: &self.guide,
            recommended_tools: self.recommended_tools.join(", "),
            next_hint: &self._next_hint,
        })
    }
}

pub fn render_protocol_guide(receipt: &contracts::GuideReceipt) -> Result<String, ErrorData> {
    receipt.render_markdown()
}

struct ReportSummaryRow<'a> {
    doc_id: &'a str,
    title: &'a str,
    current_rev: i64,
    block_count: i64,
    updated_at: String,
    session_id: Option<&'a str>,
}

fn compact_timestamp(raw: &str) -> String {
    // Convert `2026-11-24T16:31:37.829618+00:00` -> `2026-11-24 16:31`
    if raw.len() >= 16 && raw.as_bytes().get(10) == Some(&b'T') {
        let mut s = raw[..16].to_string();
        s.replace_range(10..11, " ");
        s
    } else {
        raw.to_string()
    }
}

#[derive(Template)]
#[template(path = "tools/list_reports.md", escape = "none")]
struct ListReportsTemplate<'a> {
    reports: Vec<ReportSummaryRow<'a>>,
    total: i64,
    next_hint: &'a str,
}

impl contracts::ListReportsReceipt {
    pub fn render_markdown(&self) -> Result<String, ErrorData> {
        let reports = self
            .reports
            .iter()
            .map(|r| ReportSummaryRow {
                doc_id: &r.doc_id,
                title: &r.title,
                current_rev: r.current_rev,
                block_count: r.block_count,
                updated_at: compact_timestamp(&r.updated_at),
                session_id: r.session_id.as_deref(),
            })
            .collect();
        render_template(&ListReportsTemplate {
            reports,
            total: self.total,
            next_hint: &self._next_hint,
        })
    }
}

pub fn render_list_reports(receipt: &contracts::ListReportsReceipt) -> Result<String, ErrorData> {
    receipt.render_markdown()
}

struct DigestBlockRow<'a> {
    id: &'a str,
    short_type: &'static str,
    char_count: Option<usize>,
    preview: Option<&'a str>,
    source_ids: String,
}

fn short_block_type(block_type: &str, level: Option<u8>) -> &'static str {
    match block_type {
        "heading" => match level {
            Some(1) => "h1",
            Some(2) => "h2",
            Some(3) => "h3",
            Some(4) => "h4",
            Some(5) => "h5",
            _ => "h6",
        },
        "paragraph" => "p",
        "quote" => "quote",
        "table" => "tbl",
        "echarts" => "chart",
        "custom_html" => "html",
        _ => "blk",
    }
}

#[derive(Template)]
#[template(path = "tools/get_document_digest.md", escape = "none")]
struct DocumentDigestTemplate<'a> {
    title: &'a str,
    current_rev: i64,
    block_count: usize,
    paragraph_count: usize,
    prose_char_count: usize,
    target_prose_chars: Option<u32>,
    prose_char_delta: Option<i64>,
    visual_block_count: usize,
    cited_source_count: usize,
    source_count: usize,
    theme_preset: &'a str,
    toc_enabled: bool,
    toc_max_depth: u8,
    blocks: Vec<DigestBlockRow<'a>>,
    next_hint: &'a str,
}

impl contracts::DigestReceipt {
    pub fn render_markdown(&self) -> Result<String, ErrorData> {
        let d = &self.digest;
        let blocks = d
            .blocks
            .iter()
            .map(|block| DigestBlockRow {
                id: &block.id,
                short_type: short_block_type(&block.block_type, block.level),
                char_count: block.char_count,
                preview: block.preview.as_deref(),
                source_ids: block
                    .source_ids
                    .iter()
                    .map(|id| format!("^{id}"))
                    .collect::<Vec<_>>()
                    .join(" "),
            })
            .collect();
        render_template(&DocumentDigestTemplate {
            title: &d.title,
            current_rev: d.current_rev,
            block_count: d.block_count,
            paragraph_count: d.paragraph_count,
            prose_char_count: d.prose_char_count,
            target_prose_chars: d.target_prose_chars.map(u32::from),
            prose_char_delta: d.prose_char_delta,
            visual_block_count: d.visual_block_count,
            cited_source_count: d.cited_source_count,
            source_count: d.source_count,
            theme_preset: &d.theme.preset,
            toc_enabled: d.toc.enabled,
            toc_max_depth: d.toc.max_depth,
            blocks,
            next_hint: &self._next_hint,
        })
    }
}

pub fn render_document_digest(receipt: &contracts::DigestReceipt) -> Result<String, ErrorData> {
    receipt.render_markdown()
}

struct LintDiagnosticRow<'a> {
    block_id: Option<&'a str>,
    message: &'a str,
    hint: &'a str,
    valid_values: String,
}

pub(super) struct LintDiagnosticGroup<'a> {
    severity: &'static str,
    code: &'a str,
    rule: &'a str,
    shared_hint: &'a str,
    shared_valid_values: String,
    compact_targets: String,
    items: Vec<LintDiagnosticRow<'a>>,
}

fn extract_compact_targets(items: &[LintDiagnosticRow<'_>]) -> String {
    if items.len() < 2 || items.iter().any(|i| i.block_id.is_some()) {
        return String::new();
    }
    let Some((first_item, rest)) = items.split_first() else {
        return String::new();
    };
    let first = first_item.message;
    let mut prefix_len = first.len();
    for item in rest {
        let m = item.message;
        prefix_len = first
            .char_indices()
            .zip(m.char_indices())
            .take_while(|((_, a), (_, b))| a == b)
            .last()
            .map_or(0, |((idx, ch), _)| idx + ch.len_utf8())
            .min(prefix_len);
    }
    let mut suffix_len = 0usize;
    let Some(first_rem) = first.get(prefix_len..) else {
        return String::new();
    };
    let first_rev: Vec<char> = first_rem.chars().rev().collect();
    let mut max_suffix_chars = first_rev.len();
    for item in rest {
        let Some(m_rem) = item.message.get(prefix_len..) else {
            return String::new();
        };
        let common_chars = first_rev
            .iter()
            .zip(m_rem.chars().rev())
            .take_while(|(a, b)| *a == b)
            .count();
        max_suffix_chars = max_suffix_chars.min(common_chars);
    }
    if max_suffix_chars > 0 {
        suffix_len = first_rem
            .char_indices()
            .rev()
            .nth(max_suffix_chars - 1)
            .map_or(0, |(idx, _)| first_rem.len() - idx);
    }
    // Snap prefix_len back to a whitespace or backtick boundary so shared prefixes like `src_` stay part of the token.
    while prefix_len > 0 {
        let prev = first[..prefix_len].chars().next_back();
        if matches!(prev, Some(' ' | '\t' | '`')) {
            break;
        }
        prefix_len -= prev.map_or(1, char::len_utf8);
    }
    while suffix_len > 0 {
        let suf_start = first.len() - suffix_len;
        let next = first[suf_start..].chars().next();
        if matches!(next, Some(' ' | '\t' | '`')) {
            break;
        }
        suffix_len -= next.map_or(1, char::len_utf8);
    }
    if prefix_len == 0 && suffix_len == 0 {
        return String::new();
    }
    let mut targets = Vec::with_capacity(items.len());
    for item in items {
        let m = item.message;
        let Some(end) = m.len().checked_sub(suffix_len) else {
            return String::new();
        };
        if end <= prefix_len {
            return String::new();
        }
        let Some(slice) = m.get(prefix_len..end) else {
            return String::new();
        };
        let mid = slice.trim().trim_matches('`');
        if mid.is_empty() || mid.contains(char::is_whitespace) {
            return String::new();
        }
        targets.push(mid);
    }
    targets.join(", ")
}

fn group_diagnostics(diagnostics: &[Diagnostic]) -> Vec<LintDiagnosticGroup<'_>> {
    let mut groups: Vec<LintDiagnosticGroup<'_>> = Vec::new();
    for d in diagnostics {
        let severity = match d.severity {
            Severity::Error => "error",
            Severity::Warning => "warning",
            Severity::Info => "info",
        };
        let item = LintDiagnosticRow {
            block_id: d.block_id.as_ref().map(|id| id.as_str()),
            message: &d.message,
            hint: &d.hint,
            valid_values: d.valid_values.join(", "),
        };
        if let Some(group) = groups
            .iter_mut()
            .find(|g| g.severity == severity && g.code == d.code.as_str())
        {
            group.items.push(item);
        } else {
            groups.push(LintDiagnosticGroup {
                severity,
                code: &d.code,
                rule: &d.rule,
                shared_hint: "",
                shared_valid_values: String::new(),
                compact_targets: String::new(),
                items: vec![item],
            });
        }
    }

    for group in &mut groups {
        if let Some(first) = group.items.first() {
            if !first.hint.is_empty() && group.items.iter().all(|item| item.hint == first.hint) {
                group.shared_hint = first.hint;
            }
            if !first.valid_values.is_empty()
                && group
                    .items
                    .iter()
                    .all(|item| item.valid_values == first.valid_values)
            {
                group.shared_valid_values = first.valid_values.clone();
            }
        }
        group.compact_targets = extract_compact_targets(&group.items);
    }
    groups
}

struct SuppressedRuleGroup<'a> {
    code: String,
    rule: &'a str,
    count: usize,
    block_ids: String,
}

#[derive(Template)]
#[template(path = "tools/lint_document.md", escape = "none")]
struct LintDocumentTemplate<'a> {
    clean: bool,
    rev: i64,
    ok: bool,
    errors: usize,
    warnings: usize,
    infos: usize,
    suppressed: usize,
    groups: Vec<LintDiagnosticGroup<'a>>,
    suppressed_groups: Vec<SuppressedRuleGroup<'a>>,
    next_hint: Option<&'a str>,
}

impl contracts::LintReceipt {
    pub fn render_markdown(&self) -> Result<String, ErrorData> {
        let clean = self.ok && self.diagnostics.is_empty() && self.suppressed_blocks.is_empty();
        let summary = self.summary.as_ref();
        let groups = group_diagnostics(&self.diagnostics);

        let mut blocks_by_code: std::collections::BTreeMap<String, Vec<&str>> =
            std::collections::BTreeMap::new();
        for (block_id, codes) in &self.suppressed_blocks {
            for code in codes {
                blocks_by_code
                    .entry(code.clone())
                    .or_default()
                    .push(block_id.as_str());
            }
        }
        let suppressed_groups = blocks_by_code
            .into_iter()
            .map(|(code, block_ids)| {
                let rule = quire_lint::RuleId::all()
                    .iter()
                    .find(|meta| meta.code == code)
                    .map_or("", |meta| meta.name);
                let count = block_ids.len();
                let block_ids = block_ids.join(", ");
                SuppressedRuleGroup {
                    code,
                    rule,
                    count,
                    block_ids,
                }
            })
            .collect();
        render_template(&LintDocumentTemplate {
            clean,
            rev: self.rev,
            ok: self.ok,
            errors: summary.map_or(0, |s| s.errors),
            warnings: summary.map_or(0, |s| s.warnings),
            infos: summary.map_or(0, |s| s.infos),
            suppressed: summary.map_or(0, |s| s.suppressed),
            groups,
            suppressed_groups,
            next_hint: self._next_hint.as_deref(),
        })
    }
}

pub fn render_lint_document(receipt: &contracts::LintReceipt) -> Result<String, ErrorData> {
    receipt.render_markdown()
}

struct CommentThreadRow<'a> {
    thread_id: &'a str,
    status: &'static str,
    block_id: &'a str,
    anchor_detached: bool,
    quote: Option<&'a str>,
    body: &'a str,
    reply_count: usize,
}

#[derive(Template)]
#[template(path = "tools/list_comments.md", escape = "none")]
struct ListCommentsTemplate<'a> {
    doc_id: &'a str,
    count: usize,
    comments: Vec<CommentThreadRow<'a>>,
    next_hint: &'a str,
}

impl contracts::ListCommentsReceipt {
    pub fn render_markdown(&self) -> Result<String, ErrorData> {
        let comments = self
            .comments
            .iter()
            .map(|thread| CommentThreadRow {
                thread_id: &thread.thread_id,
                status: match thread.status {
                    CommentStatus::Open => "open",
                    CommentStatus::Addressed => "done",
                    CommentStatus::Archived => "archived",
                },
                block_id: thread.block_id.as_str(),
                anchor_detached: thread.anchor_detached,
                quote: thread.quote.as_ref().map(|q| q.exact.as_str()),
                body: thread
                    .messages
                    .first()
                    .map_or("", |message| message.body.as_str()),
                reply_count: thread.messages.len().saturating_sub(1),
            })
            .collect();
        render_template(&ListCommentsTemplate {
            doc_id: &self.doc_id,
            count: self.count,
            comments,
            next_hint: &self._next_hint,
        })
    }
}

pub fn render_list_comments(receipt: &contracts::ListCommentsReceipt) -> Result<String, ErrorData> {
    receipt.render_markdown()
}

mod mutations;
pub use mutations::{
    render_apply_operations, render_delete_sources, render_list_sources, render_upsert_sources,
};

#[cfg(test)]
mod tests;
