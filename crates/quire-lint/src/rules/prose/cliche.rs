use super::*;

pub(super) fn check_cliche_density(context: &LintContext<'_>, engine: &mut Engine) {
    let document = context.document;
    let mut previous_transition = None;
    for (index, block) in document.blocks.iter().enumerate() {
        let BlockKind::Paragraph { .. } = &block.body else {
            previous_transition = None;
            continue;
        };
        let Some(precomputed) = context.paragraph(index) else {
            previous_transition = None;
            continue;
        };
        let masked = &precomputed.masked;
        let mut hits = BTreeSet::new();
        scan_cliche_patterns(masked, &mut hits);
        if CHAPTER_GUIDE_RE
            .as_ref()
            .is_some_and(|pattern| pattern.is_match(masked))
        {
            hits.insert("chapter-preview narration");
        }
        for phrase in ENGLISH_CLICHES {
            if contains_ascii_phrase(masked, phrase) {
                hits.insert(*phrase);
            }
        }
        for phrase in EVIDENCE_DEPENDENT_EVALUATIONS {
            if masked
                .split(['。', '！', '？', '；', '\n'])
                .any(|sentence| sentence.contains(phrase) && !has_quantitative_evidence(sentence))
            {
                hits.insert(*phrase);
            }
        }

        let leading_transition = leading_repeated_transition(masked);
        for phrase in REPEATED_TRANSITIONS {
            if masked.matches(*phrase).count() >= 2
                || (leading_transition == Some(*phrase) && previous_transition == Some(*phrase))
            {
                hits.insert(*phrase);
            }
        }
        previous_transition = leading_transition;

        if hits.is_empty() {
            continue;
        }
        let words = hits.into_iter().collect::<Vec<_>>().join("、");
        engine.report(
            RuleId::ClichePhraseDensity,
            format!(
                "block `{}` contains unsupported or mechanical phrases: {words}",
                block.id
            ),
            "In this Paragraph via apply_operations, delete empty phrases and chapter-preview narration such as enumerated viewing angles or 'first A, then B'; open directly with a quantified core judgment and cross-institution evidence, avoid repeating mechanical transitions, and support evaluative wording with a real metric, effect size or rank. Do not replace one cliché with another; quote exact policy or source terms in \"\" if needed.",
            Some(block.id.clone()),
            Some(format!("blocks[{index}].text")),
        );
    }
}

fn scan_cliche_patterns(text: &str, hits: &mut BTreeSet<&'static str>) {
    let mut jargon_hits = BTreeSet::new();
    let mut combination_punch_count = 0usize;
    match &*CLICHE_AUTOMATON {
        Ok(automaton) => {
            for matched in automaton.find_overlapping_iter(text) {
                let Some(pattern) = CLICHE_PATTERNS.get(matched.pattern().as_usize()) else {
                    continue;
                };
                if pattern.unconditional {
                    hits.insert(pattern.phrase);
                } else {
                    jargon_hits.insert(pattern.phrase);
                }
                if pattern.phrase == "组合拳" {
                    combination_punch_count = combination_punch_count.saturating_add(1);
                }
            }
        }
        Err(_) => {
            // A builder failure must not disable a publication rule. Tests make this path
            // unreachable for the built-in patterns; the linear fallback keeps behavior safe.
            for pattern in CLICHE_PATTERNS.iter() {
                if !text.contains(pattern.phrase) {
                    continue;
                }
                if pattern.unconditional {
                    hits.insert(pattern.phrase);
                } else {
                    jargon_hits.insert(pattern.phrase);
                }
                if pattern.phrase == "组合拳" {
                    combination_punch_count = combination_punch_count.saturating_add(1);
                }
            }
        }
    }
    if contains_unguarded_eco(text) {
        jargon_hits.insert("生态");
    }
    if jargon_hits.len() >= 2 {
        hits.extend(jargon_hits);
    }
    if combination_punch_count >= 2 {
        hits.insert("组合拳");
    }
}

fn leading_repeated_transition(text: &str) -> Option<&'static str> {
    let trimmed = text.trim_start();
    REPEATED_TRANSITIONS
        .iter()
        .copied()
        .find(|phrase| trimmed.starts_with(phrase))
}

pub(super) fn check_low_evidence(context: &LintContext<'_>, engine: &mut Engine) {
    let document = context.document;
    for (index, block) in document.blocks.iter().enumerate() {
        let BlockKind::Paragraph { text } = &block.body else {
            continue;
        };
        let Some(precomputed) = context.paragraph(index) else {
            continue;
        };
        if text.chars().any(|ch| ch.is_ascii_digit())
            || precomputed.has_inline_citation
            || !block.source_ids.is_empty()
            || precomputed.has_book_title
        {
            continue;
        }
        let length = precomputed.masked_plain_chars;
        if length < 120 {
            continue;
        }
        engine.report(
            RuleId::LowEvidenceProse,
            format!(
                "block `{}` has {length} prose characters but no number, year, metric or citation",
                block.id
            ),
            "In this Paragraph via apply_operations, add a real metric paired with its `[^source_id]` already retrieved by tools, or cite an official policy title (`《...》`), or cut the unsupported prose; never convert a Chinese numeral (`五项`) to an uncited Arabic digit (`5项`, which triggers QR103) and never invent figures or citations to satisfy the check.",
            Some(block.id.clone()),
            Some(format!("blocks[{index}].text")),
        );
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn cliche_automaton_is_valid_and_preserves_overlapping_hits() {
        assert!(CLICHE_AUTOMATON.is_ok());
        let mut hits = BTreeSet::new();
        scan_cliche_patterns("以上判断仅供参考，组合拳与组合拳并用", &mut hits);
        assert!(hits.contains("以上判断仅供参考"));
        assert!(hits.contains("仅供参考"));
        assert!(hits.contains("组合拳"));
    }
}
