//! Diagnostic triage and compact `_next_hint` orchestration.

use std::collections::{BTreeMap, BTreeSet};

use quire_protocol::{BlockId, Diagnostic, LintReport, Severity};

const MAX_EXPANDED_GROUPS: usize = 5;

struct HintGroup<'a> {
    block_id: Option<&'a BlockId>,
    codes: Vec<(&'a str, u8)>,
    priority: u8,
    total: usize,
}

/// Priority tiers are ordered by expected repair leverage, not by severity alone.
pub(super) fn priority_tier(diagnostic: &Diagnostic) -> u8 {
    match diagnostic.severity {
        Severity::Error => 0,
        Severity::Info => 4,
        Severity::Warning => warning_tier(&diagnostic.code),
    }
}

fn warning_tier(code: &str) -> u8 {
    let numeric = code
        .strip_prefix("QR")
        .and_then(|digits| digits.parse::<u16>().ok());
    match numeric {
        Some(417..=419) => 2,
        Some(412..=415) | Some(421 | 423 | 426) => 3,
        _ => 1,
    }
}

/// Group diagnostics by block, order groups by repair leverage, and cap the
/// expanded list so a single `_next_hint` remains readable for lightweight models.
pub(super) fn summarize_diagnostics_for_hint(diagnostics: &[Diagnostic]) -> String {
    if diagnostics.is_empty() {
        return String::new();
    }

    let mut grouped: BTreeMap<Option<&BlockId>, Vec<&Diagnostic>> = BTreeMap::new();
    let mut warning_blocks: BTreeMap<&str, BTreeSet<&BlockId>> = BTreeMap::new();
    for diagnostic in diagnostics {
        grouped
            .entry(diagnostic.block_id.as_ref())
            .or_default()
            .push(diagnostic);
        if diagnostic.severity == Severity::Warning
            && let Some(block_id) = diagnostic.block_id.as_ref()
        {
            warning_blocks
                .entry(diagnostic.code.as_str())
                .or_default()
                .insert(block_id);
        }
    }

    let mut groups: Vec<HintGroup<'_>> = grouped
        .into_iter()
        .map(|(block_id, items)| {
            let mut code_tiers: BTreeMap<&str, u8> = BTreeMap::new();
            for diagnostic in &items {
                let tier = priority_tier(diagnostic);
                code_tiers
                    .entry(diagnostic.code.as_str())
                    .and_modify(|current| *current = (*current).min(tier))
                    .or_insert(tier);
            }
            let mut codes: Vec<(&str, u8)> = code_tiers.into_iter().collect();
            codes.sort_by_key(|(code, tier)| (*tier, *code));
            let priority = codes.first().map_or(4, |entry| entry.1);
            HintGroup {
                block_id,
                codes,
                priority,
                total: items.len(),
            }
        })
        .collect();

    groups.sort_by(|left, right| {
        left.priority
            .cmp(&right.priority)
            .then_with(|| right.total.cmp(&left.total))
            .then_with(|| group_label(left.block_id).cmp(group_label(right.block_id)))
    });

    let total_groups = groups.len();
    let mut parts = Vec::new();
    for group in groups.iter().take(MAX_EXPANDED_GROUPS) {
        let codes = group
            .codes
            .iter()
            .map(|(code, _)| *code)
            .collect::<Vec<_>>()
            .join(", ");
        parts.push(format!("{} [{codes}]", group_label(group.block_id)));
    }
    let mut summary = parts.join("; ");
    if total_groups > MAX_EXPANDED_GROUPS {
        summary.push_str(&format!(
            " (+{} more block(s))",
            total_groups.saturating_sub(MAX_EXPANDED_GROUPS)
        ));
    }

    let high_frequency = warning_blocks
        .iter()
        .filter(|(_, blocks)| blocks.len() > 3)
        .map(|(code, _)| *code)
        .collect::<Vec<_>>();
    if !high_frequency.is_empty() {
        summary.push_str(&format!(
            " High-frequency warning(s): {}. Fix the top 5 blocks above in one apply_operations batch first, then re-run lint_document for the remaining blocks.",
            high_frequency.join(", ")
        ));
    }
    summary
}

fn group_label(block_id: Option<&BlockId>) -> &str {
    block_id.map(BlockId::as_str).unwrap_or("document")
}

/// Build the `apply_operations` hint with an explicit warning-remediation branch.
pub(super) fn apply_operations_next_hint(
    revision: i64,
    blocking_errors: &[Diagnostic],
    inline_warnings: &[Diagnostic],
    no_op: bool,
    rebased: bool,
    rebased_over: usize,
    base_revision: i64,
) -> String {
    let warnings = inline_warnings
        .iter()
        .filter(|diagnostic| diagnostic.severity == Severity::Warning)
        .cloned()
        .collect::<Vec<_>>();
    if !blocking_errors.is_empty() {
        let mut all = Vec::with_capacity(blocking_errors.len() + warnings.len());
        all.extend_from_slice(blocking_errors);
        all.extend_from_slice(&warnings);
        let summary = summarize_diagnostics_for_hint(&all);
        return format!(
            "Committed as revision {revision} with {} blocking error(s) (and {} warning(s)). Priority targets: {summary}. Fix all rules on each listed block in one apply_operations batch, then call lint_document and continue at expected_revision={revision}.",
            blocking_errors.len(),
            warnings.len()
        );
    }
    if !warnings.is_empty() {
        let summary = summarize_diagnostics_for_hint(&warnings);
        return format!(
            "Committed as revision {revision} with {} warning(s) on edited blocks: {summary}. Rewrite affected blocks in one apply_operations batch before moving on (or add suppressible codes to block.lint_ignore ONLY if intentionally required), then continue at expected_revision={revision}.",
            warnings.len()
        );
    }
    if no_op {
        return format!(
            "Batch was a no-op; no revision was created. Current revision is {revision}."
        );
    }
    if rebased {
        return format!(
            "Committed as revision {revision} after rebasing over {rebased_over} revision(s) from base {base_revision}. Continue at expected_revision={revision}."
        );
    }
    format!("Call lint_document, then continue at expected_revision={revision}.")
}

/// Build the `lint_document` hint from the same grouped diagnostic projection.
pub(super) fn lint_document_next_hint(report: &LintReport) -> String {
    let summary = summarize_diagnostics_for_hint(&report.diagnostics);
    if report.has_errors() {
        return format!(
            "Priority targets: {summary}. Fix errors first, batching co-occurring warnings on the same block in one apply_operations call."
        );
    }
    if report.summary.warnings > 0 {
        return format!(
            "Priority targets: {summary}. Fix structural/chart warnings before prose polish in one apply_operations batch per block; use block.lint_ignore only when a suppressible warning is genuinely intentional, or call read_as_markdown if ready."
        );
    }
    if report.summary.infos > 0 {
        return format!("Review info items ({summary}) or call read_as_markdown.");
    }
    "No active diagnostics; call read_as_markdown for a final read-through if needed.".to_string()
}

#[cfg(test)]
mod tests {
    use super::*;
    use quire_protocol::{LintSummary, Severity};

    fn block_id(raw: &str) -> Result<BlockId, Box<dyn std::error::Error>> {
        Ok(BlockId::parse(raw)?)
    }

    fn diagnostic(code: &str, severity: Severity, block_id: Option<BlockId>) -> Diagnostic {
        Diagnostic {
            code: code.to_string(),
            rule: String::new(),
            severity,
            message: String::new(),
            hint: String::new(),
            suppressible: true,
            block_id,
            path: None,
            valid_values: Vec::new(),
        }
    }

    #[test]
    fn priority_tiers_follow_repair_leverage() -> Result<(), Box<dyn std::error::Error>> {
        let error = diagnostic("QR101", Severity::Error, Some(block_id("b_error")?));
        let chart = diagnostic("QR316", Severity::Warning, Some(block_id("b_chart")?));
        let visuals = diagnostic("QR416", Severity::Warning, Some(block_id("b_visuals")?));
        let layout = diagnostic("QR417", Severity::Warning, Some(block_id("b_layout")?));
        let heading = diagnostic("QR418", Severity::Warning, Some(block_id("b_heading")?));
        let dense_prose = diagnostic("QR419", Severity::Warning, Some(block_id("b_dense_prose")?));
        let prose = diagnostic("QR412", Severity::Warning, Some(block_id("b_prose")?));
        let prose_fourteen = diagnostic("QR414", Severity::Warning, Some(block_id("b_prose_14")?));
        let blind_ordinal = diagnostic("QR421", Severity::Warning, Some(block_id("b_blind")?));
        let visual_note = diagnostic("QR422", Severity::Warning, Some(block_id("b_visual_note")?));
        let policy_format = diagnostic("QR423", Severity::Warning, Some(block_id("b_policy")?));
        let info = diagnostic("QR103", Severity::Info, Some(block_id("b_info")?));
        assert_eq!(priority_tier(&error), 0);
        assert_eq!(priority_tier(&chart), 1);
        assert_eq!(priority_tier(&visuals), 1);
        assert_eq!(priority_tier(&layout), 2);
        assert_eq!(priority_tier(&heading), 2);
        assert_eq!(priority_tier(&dense_prose), 2);
        assert_eq!(priority_tier(&prose), 3);
        assert_eq!(priority_tier(&prose_fourteen), 3);
        assert_eq!(priority_tier(&blind_ordinal), 3);
        assert_eq!(priority_tier(&policy_format), 3);
        assert_eq!(priority_tier(&visual_note), 1);
        assert_eq!(priority_tier(&info), 4);
        Ok(())
    }

    #[test]
    fn summary_groups_and_orders_by_tier() -> Result<(), Box<dyn std::error::Error>> {
        let diagnostics = vec![
            diagnostic("QR103", Severity::Info, Some(block_id("b_info")?)),
            diagnostic("QR412", Severity::Warning, Some(block_id("b_prose")?)),
            diagnostic("QR417", Severity::Warning, Some(block_id("b_layout")?)),
            diagnostic("QR316", Severity::Warning, Some(block_id("b_chart")?)),
            diagnostic("QR101", Severity::Error, Some(block_id("b_error")?)),
        ];
        assert_eq!(
            summarize_diagnostics_for_hint(&diagnostics),
            "b_error [QR101]; b_chart [QR316]; b_layout [QR417]; b_prose [QR412]; b_info [QR103]"
        );
        Ok(())
    }

    #[test]
    fn summary_merges_codes_for_the_same_block() -> Result<(), Box<dyn std::error::Error>> {
        let id = block_id("b_12")?;
        let diagnostics = vec![
            diagnostic("QR412", Severity::Warning, Some(id.clone())),
            diagnostic("QR417", Severity::Warning, Some(id.clone())),
            diagnostic("QR417", Severity::Warning, Some(id)),
        ];
        assert_eq!(
            summarize_diagnostics_for_hint(&diagnostics),
            "b_12 [QR417, QR412]"
        );
        Ok(())
    }

    #[test]
    fn summary_caps_expanded_blocks_and_counts_the_rest() -> Result<(), Box<dyn std::error::Error>>
    {
        let mut diagnostics = Vec::new();
        for index in 1..=6 {
            let id = block_id(format!("b_{index}").as_str())?;
            diagnostics.push(diagnostic("QR316", Severity::Warning, Some(id)));
        }
        let summary = summarize_diagnostics_for_hint(&diagnostics);
        assert!(summary.contains("(+1 more block(s))"), "{summary}");
        assert!(!summary.contains("b_6 ["), "{summary}");
        Ok(())
    }

    #[test]
    fn summary_suggests_batching_high_frequency_warnings() -> Result<(), Box<dyn std::error::Error>>
    {
        let mut diagnostics = Vec::new();
        for index in 1..=4 {
            let id = block_id(format!("b_{index}").as_str())?;
            diagnostics.push(diagnostic("QR412", Severity::Warning, Some(id)));
        }
        let summary = summarize_diagnostics_for_hint(&diagnostics);
        assert!(
            summary.contains("High-frequency warning(s): QR412"),
            "{summary}"
        );
        assert!(
            summary.contains(
                "Fix the top 5 blocks above in one apply_operations batch first, then re-run lint_document for the remaining blocks."
            ),
            "{summary}"
        );
        assert!(!summary.contains("select=["), "{summary}");
        Ok(())
    }

    #[test]
    fn apply_operations_warns_about_inline_warnings_instead_of_continuing()
    -> Result<(), Box<dyn std::error::Error>> {
        let warning = diagnostic("QR417", Severity::Warning, Some(block_id("b_12")?));
        let hint = apply_operations_next_hint(7, &[], &[warning], false, false, 0, 7);
        assert!(hint.contains("b_12"), "{hint}");
        assert!(hint.contains("expected_revision=7"), "{hint}");
        assert!(hint.contains("warning(s) on edited blocks"), "{hint}");
        assert!(!hint.starts_with("Call lint_document"), "{hint}");
        Ok(())
    }

    #[test]
    fn apply_operations_keeps_noop_hint_for_info_only() -> Result<(), Box<dyn std::error::Error>> {
        let info = diagnostic("QR103", Severity::Info, Some(block_id("b_12")?));
        let hint = apply_operations_next_hint(5, &[], &[info], true, false, 0, 5);
        assert!(hint.contains("no-op"), "{hint}");
        Ok(())
    }

    #[test]
    fn lint_hint_uses_errors_first_then_warning_branch() -> Result<(), Box<dyn std::error::Error>> {
        let error = diagnostic("QR101", Severity::Error, Some(block_id("b_error")?));
        let error_report = LintReport {
            diagnostics: vec![error],
            summary: LintSummary {
                errors: 1,
                warnings: 1,
                ..LintSummary::default()
            },
        };
        assert!(lint_document_next_hint(&error_report).contains("Fix errors first"));

        let warning = diagnostic("QR417", Severity::Warning, Some(block_id("b_layout")?));
        let warning_report = LintReport {
            diagnostics: vec![warning],
            summary: LintSummary {
                errors: 0,
                warnings: 1,
                suppressed: 2,
                ..LintSummary::default()
            },
        };
        let hint = lint_document_next_hint(&warning_report);
        assert!(hint.contains("b_layout"), "{hint}");
        assert!(!hint.contains("suppressed"), "{hint}");
        assert!(hint.contains("Priority targets:"), "{hint}");

        let clean_report = LintReport {
            diagnostics: vec![],
            summary: LintSummary {
                errors: 0,
                warnings: 0,
                infos: 0,
                suppressed: 5,
            },
        };
        let clean_hint = lint_document_next_hint(&clean_report);
        assert!(!clean_hint.contains("Review info items"), "{clean_hint}");
        assert!(clean_hint.contains("No active diagnostics"), "{clean_hint}");
        Ok(())
    }
}
