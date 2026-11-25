//! Canonical Markdown rendering for report tool receipts shared across native Rig tools and rmcp.

use askama::Template;
use quire_protocol::{CommentStatus, Diagnostic, Severity};
use rmcp::ErrorData;

use crate::{guide::GuideTopic, report::schema as contracts};

fn render_template<T: Template>(template: &T) -> Result<String, ErrorData> {
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

struct LintDiagnosticGroup<'a> {
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

#[derive(Template)]
#[template(path = "tools/upsert_sources.md", escape = "none")]
struct UpsertSourcesTemplate<'a> {
    doc_id: &'a str,
    revision: i64,
    changed: bool,
    source_count: usize,
    remapped: String,
    groups: Vec<LintDiagnosticGroup<'a>>,
    next_hint: &'a str,
}

impl contracts::UpsertSourcesReceipt {
    pub fn render_markdown(&self) -> Result<String, ErrorData> {
        let remapped = self
            .canonical_id_map
            .iter()
            .filter(|(from, to)| from != to)
            .map(|(from, to)| format!("{from}->{to}"))
            .collect::<Vec<_>>()
            .join(", ");
        let groups = group_diagnostics(&self.inline_warnings);
        render_template(&UpsertSourcesTemplate {
            doc_id: &self.doc_id,
            revision: self.revision,
            changed: self.changed,
            source_count: self.source_count,
            remapped,
            groups,
            next_hint: &self._next_hint,
        })
    }
}

pub fn render_upsert_sources(
    receipt: &contracts::UpsertSourcesReceipt,
) -> Result<String, ErrorData> {
    receipt.render_markdown()
}

#[derive(Template)]
#[template(path = "tools/delete_sources.md", escape = "none")]
struct DeleteSourcesTemplate<'a> {
    doc_id: &'a str,
    revision: i64,
    changed: bool,
    deleted_ids: &'a [String],
    deleted_ids_str: String,
    source_count: usize,
    next_hint: &'a str,
}

impl contracts::DeleteSourcesReceipt {
    pub fn render_markdown(&self) -> Result<String, ErrorData> {
        render_template(&DeleteSourcesTemplate {
            doc_id: &self.doc_id,
            revision: self.revision,
            changed: self.changed,
            deleted_ids: &self.deleted_ids,
            deleted_ids_str: self.deleted_ids.join(", "),
            source_count: self.source_count,
            next_hint: &self._next_hint,
        })
    }
}

pub fn render_delete_sources(
    receipt: &contracts::DeleteSourcesReceipt,
) -> Result<String, ErrorData> {
    receipt.render_markdown()
}

struct SourceRow<'a> {
    id: &'a str,
    title: &'a str,
    url: Option<&'a str>,
    date: Option<&'a str>,
    note: Option<&'a str>,
    excerpt: Option<&'a str>,
}

#[derive(Template)]
#[template(path = "tools/list_sources.md", escape = "none")]
struct ListSourcesTemplate<'a> {
    doc_id: &'a str,
    revision: i64,
    count: usize,
    sources: Vec<SourceRow<'a>>,
    next_hint: &'a str,
}

impl contracts::ListSourcesReceipt {
    pub fn render_markdown(&self) -> Result<String, ErrorData> {
        let sources = self
            .sources
            .iter()
            .map(|(id, entry)| SourceRow {
                id: id.as_str(),
                title: &entry.title,
                url: entry.url.as_deref(),
                date: entry.date.as_deref(),
                note: entry.note.as_deref(),
                excerpt: entry.excerpt.as_deref(),
            })
            .collect();
        render_template(&ListSourcesTemplate {
            doc_id: &self.doc_id,
            revision: self.revision,
            count: self.count,
            sources,
            next_hint: &self._next_hint,
        })
    }
}

pub fn render_list_sources(receipt: &contracts::ListSourcesReceipt) -> Result<String, ErrorData> {
    receipt.render_markdown()
}

#[derive(Template)]
#[template(path = "tools/apply_operations.md", escape = "none")]
struct ApplyOperationsTemplate<'a> {
    base_revision: i64,
    new_revision: i64,
    rebased: bool,
    rebased_over: usize,
    assigned_ids: String,
    error_groups: Vec<LintDiagnosticGroup<'a>>,
    warning_groups: Vec<LintDiagnosticGroup<'a>>,
    next_hint: &'a str,
}

impl contracts::ApplyOperationsReceipt {
    pub fn render_markdown(&self) -> Result<String, ErrorData> {
        let assigned_ids = self
            .assigned_ids
            .iter()
            .map(|a| {
                format!(
                    "{}->{}",
                    a.requested.as_ref().map_or("<auto>", |id| id.as_str()),
                    a.assigned
                )
            })
            .collect::<Vec<_>>()
            .join(", ");
        let error_groups = group_diagnostics(&self.blocking_errors);
        let warning_groups = group_diagnostics(&self.inline_warnings);
        render_template(&ApplyOperationsTemplate {
            base_revision: self.base_revision,
            new_revision: self.new_revision,
            rebased: self.rebased,
            rebased_over: self.rebased_over,
            assigned_ids,
            error_groups,
            warning_groups,
            next_hint: &self._next_hint,
        })
    }
}

pub fn render_apply_operations(
    receipt: &contracts::ApplyOperationsReceipt,
) -> Result<String, ErrorData> {
    receipt.render_markdown()
}

#[cfg(test)]
mod tests {
    use std::collections::BTreeMap;

    use indexmap::IndexMap;
    use quire_protocol::{BlockId, Diagnostic, LintSummary, SourceEntry, SourceId};

    use super::*;

    #[test]
    fn read_as_markdown_renders_unencoded_markdown_and_hint() {
        let receipt = contracts::ReadMarkdownReceipt {
            source: "doc:demo".into(),
            doc_id: "demo".into(),
            revision: 3,
            markdown_chars: 42,
            markdown: "## 章节标题\n\n正文包含 \"引号\" 与 <citation id=\"src_1\"/>。".into(),
            _next_hint: "Patch only changed blocks.".into(),
        };
        let rendered = render_read_as_markdown(&receipt).expect("render read_as_markdown");
        assert!(rendered.contains("doc_id: demo | rev: 3 | chars: 42"));
        assert!(rendered.contains("\"引号\""));
        assert!(rendered.contains("<citation id=\"src_1\"/>"));
        assert!(rendered.contains("> Next: Patch only changed blocks."));
        assert!(!rendered.contains("\\n"));
    }

    #[test]
    fn lint_document_renders_clean_and_diagnostic_states() {
        let clean = contracts::LintReceipt {
            ok: true,
            rev: 5,
            summary: None,
            suppressed_blocks: BTreeMap::new(),
            diagnostics: Vec::new(),
            _next_hint: None,
        };
        let clean_text = render_lint_document(&clean).expect("render clean lint");
        assert_eq!(clean_text.trim(), "OK — rev=5, 0 diagnostics.");

        let dirty = contracts::LintReceipt {
            ok: false,
            rev: 6,
            summary: Some(LintSummary {
                errors: 1,
                warnings: 0,
                infos: 0,
                suppressed: 0,
            }),
            suppressed_blocks: BTreeMap::new(),
            diagnostics: vec![Diagnostic {
                code: "QR201".into(),
                rule: "missing-source".into(),
                severity: Severity::Error,
                message: "Unregistered source".into(),
                hint: "Call upsert_sources first.".into(),
                suppressible: false,
                block_id: Some(BlockId::parse("sec_1").expect("valid block id")),
                path: None,
                valid_values: vec!["src_a".into()],
            }],
            _next_hint: Some("Fix errors first.".into()),
        };
        let dirty_text = render_lint_document(&dirty).expect("render dirty lint");
        assert!(dirty_text.contains("[error] QR201 (missing-source) @sec_1"));
        assert!(dirty_text.contains("Call upsert_sources first."));
        assert!(dirty_text.contains("> Next: Fix errors first."));

        let grouped = contracts::LintReceipt {
            ok: true,
            rev: 7,
            summary: Some(LintSummary {
                errors: 0,
                warnings: 2,
                infos: 0,
                suppressed: 0,
            }),
            suppressed_blocks: BTreeMap::new(),
            diagnostics: vec![
                Diagnostic {
                    code: "QR102".into(),
                    rule: "unused-source".into(),
                    severity: Severity::Warning,
                    message: "source `src_a` is registered but never cited".into(),
                    hint: "add [^<source_id>] to prose".into(),
                    suppressible: true,
                    block_id: None,
                    path: Some("sources.src_a".into()),
                    valid_values: Vec::new(),
                },
                Diagnostic {
                    code: "QR102".into(),
                    rule: "unused-source".into(),
                    severity: Severity::Warning,
                    message: "source `src_b` is registered but never cited".into(),
                    hint: "add [^<source_id>] to prose".into(),
                    suppressible: true,
                    block_id: None,
                    path: Some("sources.src_b".into()),
                    valid_values: Vec::new(),
                },
            ],
            _next_hint: None,
        };
        let grouped_text = render_lint_document(&grouped).expect("render grouped lint");
        assert!(grouped_text.contains("[warning] QR102 (unused-source) × 2"));
        assert!(grouped_text.contains("Targets: src_a, src_b"));
        assert!(!grouped_text.contains("registered but never cited"));
        assert_eq!(
            grouped_text.matches("add [^<source_id>] to prose").count(),
            1
        );

        let mut suppressed_blocks = BTreeMap::new();
        suppressed_blocks.insert("c12_p05".into(), vec!["QR423".into()]);
        suppressed_blocks.insert("c21_p01".into(), vec!["QR423".into()]);
        let suppressed_only = contracts::LintReceipt {
            ok: true,
            rev: 70,
            summary: Some(LintSummary {
                errors: 0,
                warnings: 0,
                infos: 0,
                suppressed: 2,
            }),
            suppressed_blocks,
            diagnostics: Vec::new(),
            _next_hint: None,
        };
        let sup_text = render_lint_document(&suppressed_only).expect("render suppressed lint");
        assert!(sup_text.contains("QR423 (higher-ed-policy-term-format) × 2: c12_p05, c21_p01"));
    }

    #[test]
    fn upsert_list_sources_and_apply_operations_render_compactly() {
        let mut canonical_id_map = BTreeMap::new();
        canonical_id_map.insert("a".into(), "a".into());
        canonical_id_map.insert("b_dup".into(), "b".into());
        let upsert = contracts::UpsertSourcesReceipt {
            doc_id: "doc_1".into(),
            revision: 4,
            changed: true,
            canonical_id_map,
            source_count: 2,
            inline_warnings: Vec::new(),
            _next_hint: "Cite sources.".into(),
        };
        let up_text = render_upsert_sources(&upsert).expect("render upsert_sources");
        assert!(up_text.contains("OK — doc_1 rev=4 (changed=true, total=2)"));
        assert!(up_text.contains("Mapped: b_dup->b"));

        let mut sources = IndexMap::new();
        sources.insert(
            SourceId::parse("moe_26").unwrap(),
            SourceEntry::new("教育部工作要点"),
        );
        let list = contracts::ListSourcesReceipt {
            doc_id: "doc_1".into(),
            revision: 4,
            sources,
            count: 1,
            _next_hint: "Verify claims.".into(),
        };
        let list_text = render_list_sources(&list).expect("render list_sources");
        assert!(list_text.contains("- [^moe_26] 教育部工作要点"));

        let apply = contracts::ApplyOperationsReceipt {
            new_revision: 5,
            base_revision: 4,
            rebased: false,
            rebased_over: 0,
            committed: true,
            assigned_ids: Vec::new(),
            inline_warnings: Vec::new(),
            blocking_errors: Vec::new(),
            _next_hint: "Continue.".into(),
        };
        let apply_text = render_apply_operations(&apply).expect("render apply_operations");
        assert!(apply_text.contains("OK — rev 4 -> 5"));

        let del = contracts::DeleteSourcesReceipt {
            doc_id: "doc_1".into(),
            revision: 6,
            changed: true,
            deleted_ids: vec!["old_src".into()],
            source_count: 1,
            _next_hint: "Run lint_document.".into(),
        };
        let del_text = render_delete_sources(&del).expect("render delete_sources");
        assert!(del_text.contains("OK — doc_1 rev=6 (changed=true, deleted=1, remaining=1)"));
        assert!(del_text.contains("Deleted: old_src"));
    }
}
