use askama::Template;
use quire_protocol::{CommentStatus, Severity};
use quire_server::{guide::GuideTopic, report as contracts};
use rig::tool::ToolExecutionError;

fn render_template<T: Template>(template: &T) -> Result<String, ToolExecutionError> {
    template
        .render()
        .map_err(|error| ToolExecutionError::other(error.to_string()))
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

pub(super) fn render_read_as_markdown(
    receipt: &contracts::ReadMarkdownReceipt,
) -> Result<String, ToolExecutionError> {
    render_template(&ReadAsMarkdownTemplate {
        doc_id: &receipt.doc_id,
        revision: receipt.revision,
        markdown_chars: receipt.markdown_chars,
        markdown: &receipt.markdown,
        next_hint: &receipt._next_hint,
    })
}

#[derive(Template)]
#[template(path = "tools/get_protocol_guide.md", escape = "none")]
struct GetProtocolGuideTemplate<'a> {
    topic: &'static str,
    guide: &'a str,
    recommended_tools: String,
    next_hint: &'a str,
}

pub(super) fn render_protocol_guide(
    receipt: &contracts::GuideReceipt,
) -> Result<String, ToolExecutionError> {
    let topic = match receipt.topic {
        GuideTopic::Quickstart => "quickstart",
        GuideTopic::TablesAndCharts => "tables_and_charts",
        GuideTopic::CitationsAndCss => "citations_and_css",
        GuideTopic::ParallelWorkflow => "parallel_workflow",
    };
    render_template(&GetProtocolGuideTemplate {
        topic,
        guide: &receipt.guide,
        recommended_tools: receipt.recommended_tools.join(", "),
        next_hint: &receipt._next_hint,
    })
}

#[derive(Template)]
#[template(path = "tools/list_reports.md", escape = "none")]
struct ListReportsTemplate<'a> {
    reports: &'a [contracts::ReportSummaryView],
    total: i64,
    next_hint: &'a str,
}

pub(super) fn render_list_reports(
    receipt: &contracts::ListReportsReceipt,
) -> Result<String, ToolExecutionError> {
    render_template(&ListReportsTemplate {
        reports: &receipt.reports,
        total: receipt.total,
        next_hint: &receipt._next_hint,
    })
}

struct DigestBlockRow<'a> {
    id: &'a str,
    block_type: &'a str,
    level: Option<u8>,
    char_count: Option<usize>,
    preview: Option<&'a str>,
    source_ids: String,
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

pub(super) fn render_document_digest(
    receipt: &contracts::DigestReceipt,
) -> Result<String, ToolExecutionError> {
    let d = &receipt.digest;
    let blocks = d
        .blocks
        .iter()
        .map(|block| DigestBlockRow {
            id: &block.id,
            block_type: &block.block_type,
            level: block.level,
            char_count: block.char_count,
            preview: block.preview.as_deref(),
            source_ids: block.source_ids.join(", "),
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
        next_hint: &receipt._next_hint,
    })
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
    items: Vec<LintDiagnosticRow<'a>>,
}

struct SuppressedBlockRow<'a> {
    block_id: &'a str,
    codes: String,
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
    suppressed_blocks: Vec<SuppressedBlockRow<'a>>,
    next_hint: Option<&'a str>,
}

pub(super) fn render_lint_document(
    receipt: &contracts::LintReceipt,
) -> Result<String, ToolExecutionError> {
    let clean =
        receipt.ok && receipt.diagnostics.is_empty() && receipt.suppressed_blocks.is_empty();
    let summary = receipt.summary.as_ref();

    let mut groups: Vec<LintDiagnosticGroup<'_>> = Vec::new();
    for d in &receipt.diagnostics {
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
    }

    let suppressed_blocks = receipt
        .suppressed_blocks
        .iter()
        .map(|(block_id, codes)| SuppressedBlockRow {
            block_id,
            codes: codes.join(", "),
        })
        .collect();
    render_template(&LintDocumentTemplate {
        clean,
        rev: receipt.rev,
        ok: receipt.ok,
        errors: summary.map_or(0, |s| s.errors),
        warnings: summary.map_or(0, |s| s.warnings),
        infos: summary.map_or(0, |s| s.infos),
        suppressed: summary.map_or(0, |s| s.suppressed),
        groups,
        suppressed_blocks,
        next_hint: receipt._next_hint.as_deref(),
    })
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

pub(super) fn render_list_comments(
    receipt: &contracts::ListCommentsReceipt,
) -> Result<String, ToolExecutionError> {
    let comments = receipt
        .comments
        .iter()
        .map(|thread| CommentThreadRow {
            thread_id: &thread.thread_id,
            status: match thread.status {
                CommentStatus::Open => "open",
                CommentStatus::Addressed => "addressed",
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
        doc_id: &receipt.doc_id,
        count: receipt.count,
        comments,
        next_hint: &receipt._next_hint,
    })
}

#[cfg(test)]
mod tests {
    use std::collections::BTreeMap;

    use quire_protocol::{BlockId, Diagnostic, LintSummary};

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
        assert_eq!(clean_text.trim(), "OK — `rev: 5`, 0 diagnostics.");

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
        assert!(dirty_text.contains("[error] `QR201` (missing-source) @ `sec_1`"));
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
        assert!(grouped_text.contains("[warning] `QR102` (unused-source) × 2"));
        assert_eq!(
            grouped_text.matches("add [^<source_id>] to prose").count(),
            1
        );
    }
}
