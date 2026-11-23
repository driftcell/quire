//! Lint execution engine: collects findings, applies select/ignore layers and builds the report.

use std::collections::HashMap;

use quire_protocol::{BlockId, Diagnostic, LintReport, LintSummary, ReportDocument, Severity};

use crate::{
    context::{LintContext, PARALLEL_BLOCK_THRESHOLD},
    registry::{RuleId, rule_matches_token},
    rules,
};

/// Per-call lint filters.
///
/// `select` restricts the reported rules (empty means all). `ignore` hides suppressible rules;
/// core contract and safety rules remain visible. Tokens accept a code (`QR202`), a rule name
/// (`table-empty-column`) or a category prefix (`QR2`).
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct LintOptions {
    pub select: Vec<String>,
    pub ignore: Vec<String>,
}

#[derive(Debug, Clone)]
pub(crate) struct Finding {
    pub rule: RuleId,
    pub message: String,
    pub hint: String,
    pub block_id: Option<BlockId>,
    pub path: Option<String>,
    pub valid_values: Vec<String>,
}

pub(crate) struct Engine {
    findings: Vec<Finding>,
}

impl Engine {
    pub(crate) fn new() -> Self {
        Self {
            findings: Vec::new(),
        }
    }

    pub(crate) fn report(
        &mut self,
        rule: RuleId,
        message: impl Into<String>,
        hint: impl Into<String>,
        block_id: Option<BlockId>,
        path: Option<String>,
    ) {
        self.report_values(rule, message, hint, block_id, path, Vec::new());
    }

    pub(crate) fn report_values(
        &mut self,
        rule: RuleId,
        message: impl Into<String>,
        hint: impl Into<String>,
        block_id: Option<BlockId>,
        path: Option<String>,
        valid_values: Vec<String>,
    ) {
        self.findings.push(Finding {
            rule,
            message: message.into(),
            hint: hint.into(),
            block_id,
            path,
            valid_values,
        });
    }

    pub(crate) fn into_findings(self) -> Vec<Finding> {
        self.findings
    }
}

fn finish_report(
    document: &ReportDocument,
    findings: Vec<Finding>,
    options: &LintOptions,
) -> LintReport {
    let block_ignores: HashMap<&str, &[String]> = document
        .blocks
        .iter()
        .map(|block| (block.id.as_str(), block.lint_ignore.as_slice()))
        .collect();
    let mut diagnostics = Vec::new();
    let mut suppressed = 0usize;
    for finding in findings {
        let meta = finding.rule.meta();
        if !options.select.is_empty()
            && !options
                .select
                .iter()
                .any(|token| rule_matches_token(meta, token))
        {
            continue;
        }
        let ignored = options
            .ignore
            .iter()
            .any(|token| rule_matches_token(meta, token))
            || document
                .lint
                .ignore
                .iter()
                .any(|token| rule_matches_token(meta, token))
            || finding.block_id.as_ref().is_some_and(|block_id| {
                block_ignores.get(block_id.as_str()).is_some_and(|tokens| {
                    tokens.iter().any(|token| rule_matches_token(meta, token))
                })
            });
        if ignored && meta.suppressible {
            suppressed += 1;
            continue;
        }
        diagnostics.push(Diagnostic {
            code: meta.code.to_string(),
            rule: meta.name.to_string(),
            severity: meta.default_severity,
            message: finding.message,
            hint: finding.hint,
            suppressible: meta.suppressible,
            block_id: finding.block_id,
            path: finding.path,
            valid_values: finding.valid_values,
        });
    }
    diagnostics.sort_by(|left, right| {
        severity_rank(left.severity)
            .cmp(&severity_rank(right.severity))
            .then_with(|| left.code.cmp(&right.code))
            .then_with(|| left.block_id.cmp(&right.block_id))
            .then_with(|| left.message.cmp(&right.message))
            .then_with(|| left.path.cmp(&right.path))
            .then_with(|| left.hint.cmp(&right.hint))
            .then_with(|| left.valid_values.cmp(&right.valid_values))
    });
    let mut summary = LintSummary {
        suppressed,
        ..LintSummary::default()
    };
    for diagnostic in &diagnostics {
        match diagnostic.severity {
            Severity::Error => summary.errors += 1,
            Severity::Warning => summary.warnings += 1,
            Severity::Info => summary.infos += 1,
        }
    }
    LintReport {
        diagnostics,
        summary,
    }
}

/// Run every static check with default options.
pub fn lint_document(document: &ReportDocument) -> LintReport {
    lint_document_with_options(document, &LintOptions::default())
}

/// Run every static check with call-level select/ignore filters. Document-level
/// `document.lint.ignore` and block-level `block.lint_ignore` are always applied as well.
pub fn lint_document_with_options(document: &ReportDocument, options: &LintOptions) -> LintReport {
    lint_document_with_strategy(
        document,
        options,
        document.blocks.len() >= PARALLEL_BLOCK_THRESHOLD,
    )
}

fn lint_document_with_strategy(
    document: &ReportDocument,
    options: &LintOptions,
    parallel: bool,
) -> LintReport {
    let context = LintContext::build(document, parallel);
    let findings = rules::run(&context, parallel);
    finish_report(document, findings, options)
}

/// Validate one ECharts block in isolation before it is written to a report.
///
/// Preview tools use this to surface chart diagnostics such as `QR301` without mutating a
/// document.
pub fn lint_echarts_block(block_id: BlockId, chart: &quire_protocol::EchartsBlock) -> LintReport {
    let Ok(doc_id) = quire_protocol::DocId::parse("chart-preview") else {
        return LintReport::default();
    };
    let mut document = ReportDocument::new(doc_id);
    document.blocks.push(quire_protocol::Block::new(
        block_id,
        quire_protocol::BlockKind::Echarts(chart.clone()),
    ));
    lint_document(&document)
}

pub(crate) fn severity_rank(severity: Severity) -> u8 {
    match severity {
        Severity::Error => 0,
        Severity::Warning => 1,
        Severity::Info => 2,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::registry::RuleId;

    fn document() -> Result<ReportDocument, Box<dyn std::error::Error>> {
        Ok(ReportDocument::new(quire_protocol::DocId::parse(
            "test-doc",
        )?))
    }

    #[test]
    fn empty_document_reports_and_summarizes() -> Result<(), Box<dyn std::error::Error>> {
        let report = lint_document(&document()?);
        assert!(
            report
                .diagnostics
                .iter()
                .any(|diagnostic| diagnostic.code == "QR410")
        );
        assert_eq!(
            report.diagnostics.len(),
            report.summary.errors + report.summary.warnings + report.summary.infos
        );
        Ok(())
    }

    #[test]
    fn category_select_keeps_only_that_category() -> Result<(), Box<dyn std::error::Error>> {
        let report = lint_document_with_options(
            &document()?,
            &LintOptions {
                select: vec!["QR4".into()],
                ..LintOptions::default()
            },
        );
        assert!(
            report
                .diagnostics
                .iter()
                .all(|diagnostic| diagnostic.code.starts_with("QR4"))
        );
        assert!(!report.diagnostics.is_empty());
        Ok(())
    }

    #[test]
    fn parallel_and_serial_reports_are_identical() -> Result<(), Box<dyn std::error::Error>> {
        let mut document = document()?;
        for index in 0..60 {
            let block_id = quire_protocol::BlockId::parse(format!("p{index}"))?;
            let text = match index % 3 {
                0 => format!("综上所述，第{index}项需要合并解读。"),
                1 => "这是一个缺少证据但需要展开分析的长段落。".repeat(24),
                _ => format!("第{index}项指标增长18项。"),
            };
            let mut block = quire_protocol::Block::paragraph(block_id, text);
            if index == 1 {
                block.lint_ignore = vec!["QR413".into()];
            }
            document.blocks.push(block);
        }
        document.blocks.push(quire_protocol::Block::table(
            quire_protocol::BlockId::parse("metrics")?,
            quire_protocol::TableBlock {
                title: Some("指标表".into()),
                numbering: None,
                note: None,
                columns: vec![
                    quire_protocol::ColumnNode::leaf("name", "高校"),
                    quire_protocol::ColumnNode::leaf("count", "数量（项）"),
                ],
                rows: vec![std::collections::BTreeMap::from([
                    ("name".into(), quire_protocol::CellValue::text("某高校")),
                    ("count".into(), quire_protocol::CellValue::Integer(12)),
                ])],
                summary_rows: Vec::new(),
            },
        ));
        document.blocks.push(quire_protocol::Block::new(
            quire_protocol::BlockId::parse("trend")?,
            quire_protocol::BlockKind::Echarts(quire_protocol::EchartsBlock {
                title: Some("趋势".into()),
                height: 320,
                option: serde_json::json!({
                    "xAxis": {"type": "category", "data": ["2025"]},
                    "yAxis": {"type": "value"},
                    "series": [{"type": "line", "data": [12]}]
                }),
                option_patch: Some(serde_json::json!({
                    "series": [{"data": [12, 18]}]
                })),
                fallback_summary: "指标由12项增至18项。".into(),
                numbering: None,
                note: Some("单位：项；2025—2026年。".into()),
            }),
        ));
        let options = LintOptions {
            select: Vec::new(),
            ignore: vec!["QR412".into()],
        };
        let serial = lint_document_with_strategy(&document, &options, false);
        assert!(!serial.diagnostics.is_empty());
        assert!(serial.summary.suppressed > 0);
        for _ in 0..8 {
            let parallel = lint_document_with_strategy(&document, &options, true);
            assert_eq!(serial, parallel);
        }
        assert_eq!(serial, lint_document_with_options(&document, &options));
        Ok(())
    }

    #[test]
    fn report_meta_is_consistent() {
        let meta = RuleId::EmptyDocument.meta();
        assert_eq!(meta.code, "QR410");
        assert_eq!(meta.name, "empty-document");
    }
}
