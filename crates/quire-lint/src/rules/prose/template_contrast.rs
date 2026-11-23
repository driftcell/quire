use super::*;

pub(super) fn check_template_parallel(context: &LintContext<'_>, engine: &mut Engine) {
    let document = context.document;
    let mut contrast_occurrences = 0usize;
    for (index, block) in document.blocks.iter().enumerate() {
        let BlockKind::Paragraph { text } = &block.body else {
            continue;
        };
        let Some(precomputed) = context.paragraph(index) else {
            continue;
        };
        let masked = &precomputed.masked;
        let has_citation_evidence = precomputed.has_inline_citation || !block.source_ids.is_empty();
        let mut reasons = Vec::new();

        if contains_in_order(masked, &["首先", "其次", "最后"])
            || contains_in_order(masked, &["首先", "再次", "最后"])
        {
            reasons.push("机械三段式连接词");
        }
        if !precomputed.has_list_structure {
            if contains_punctuated_sequence(masked, &["其一", "其二", "其三"]) {
                reasons.push("机械“其一/其二/其三”枚举");
            }
            if contains_punctuated_sequence(masked, &["第一", "第二", "第三"]) {
                reasons.push("机械“第一/第二/第三”枚举");
            }
        }
        if !precomputed.has_list_structure {
            if let Some(items) =
                ordered_enumeration_items(masked, &["一是", "二是", "三是", "四是"])
            {
                if !substantive_enumeration(masked, &items, has_citation_evidence) {
                    reasons.push("机械“一是/二是/三是/四是”枚举");
                }
            } else if let Some(items) = ordered_enumeration_items(masked, &["一是", "二是", "三是"])
                && !substantive_enumeration(masked, &items, has_citation_evidence)
            {
                reasons.push("机械“一是/二是/三是”枚举");
            }
        }
        if masked.contains("一方面") && masked.contains("另一方面") {
            contrast_occurrences = contrast_occurrences.saturating_add(1);
            if contrast_occurrences >= 3 {
                reasons.push("全篇高频“一方面/另一方面”对照");
            }
        }
        if bold_list_count(text) >= 3
            && !has_digit_outside_list_markers(text)
            && !has_citation_evidence
        {
            reasons.push("无数据支撑的加粗小标题列表");
        }
        if reasons.is_empty() {
            continue;
        }
        engine.report(
            RuleId::TemplateParallelProse,
            format!(
                "block `{}` uses an AI template: {}",
                block.id,
                reasons.join("；")
            ),
            "In this Paragraph via apply_operations, remove empty mechanical connectives or give every enumeration item a concrete subject, action, mechanism, metric or source. Substantive semicolon-delimited 一是/二是 items are valid official-document prose; do not replace them with a decorative bullet list or another synonym shell.",
            Some(block.id.clone()),
            Some(format!("blocks[{index}].text")),
        );
    }
}

fn ordered_enumeration_items<'a>(text: &'a str, markers: &[&str]) -> Option<Vec<&'a str>> {
    let mut remaining = text;
    let mut items = Vec::with_capacity(markers.len());
    for (index, marker) in markers.iter().enumerate() {
        let found = remaining.find(marker)?;
        if index > 0 {
            items.push(remaining.get(..found)?);
        }
        remaining = remaining.get(found + marker.len()..)?;
    }
    items.push(remaining);
    Some(items)
}

fn substantive_enumeration(text: &str, items: &[&str], has_citation_evidence: bool) -> bool {
    if items.is_empty() || (!has_citation_evidence && !has_quantitative_evidence(text)) {
        return false;
    }
    let lengths: Vec<usize> = items
        .iter()
        .map(|item| masked_plain_char_count(item))
        .collect();
    let total = lengths.iter().sum::<usize>();
    lengths
        .iter()
        .all(|length| *length >= ENUMERATION_MIN_ITEM_CHARS)
        && total >= ENUMERATION_AVG_ITEM_CHARS.saturating_mul(lengths.len())
}

pub(super) fn check_contrast_and_hedging(context: &LintContext<'_>, engine: &mut Engine) {
    let document = context.document;
    let mut contrast_sentences_in_document = 0usize;
    for (index, block) in document.blocks.iter().enumerate() {
        let BlockKind::Paragraph { .. } = &block.body else {
            continue;
        };
        let Some(precomputed) = context.paragraph(index) else {
            continue;
        };
        let masked = &precomputed.masked;
        let mut contrast_labels = BTreeSet::new();
        let mut contrast_sentences = 0usize;
        for sentence in masked.split(['。', '！', '？', '；', '\n']) {
            if let Some(label) = first_contrast_label(sentence) {
                contrast_sentences = contrast_sentences.saturating_add(1);
                contrast_labels.insert(label);
            }
        }
        contrast_sentences_in_document =
            contrast_sentences_in_document.saturating_add(contrast_sentences);
        let report_contrast = contrast_sentences >= 2
            || (contrast_sentences > 0 && contrast_sentences_in_document >= 3);

        let strong_hedges = strong_hedge_hits(masked);
        let light_hedges = light_hedge_hits(masked);
        let report_light_hedges = light_hedges.len() >= 2;
        let report_hedging = !strong_hedges.is_empty() || report_light_hedges;
        if !report_contrast && !report_hedging {
            continue;
        }

        let mut parts = Vec::new();
        if report_contrast && !contrast_labels.is_empty() {
            parts.push(format!(
                "对比套式：{}",
                contrast_labels.into_iter().collect::<Vec<_>>().join("、")
            ));
        }
        if report_hedging {
            let mut hedges = strong_hedges;
            if report_light_hedges {
                hedges.extend(light_hedges);
            }
            if !hedges.is_empty() {
                parts.push(format!(
                    "防御性/缓冲虚词：{}",
                    hedges.into_iter().collect::<Vec<_>>().join("、")
                ));
            }
        }
        engine.report(
            RuleId::ContrastAndHedgingTics,
            format!(
                "block `{}` uses contrast and hedging tics: {}",
                block.id,
                parts.join("；")
            ),
            "In this Paragraph via apply_operations, delete the defensive or contrast shell and state the core fact, causal mechanism or quantitative comparison directly; do not replace it with another hedge or 'not X but Y' paraphrase.",
            Some(block.id.clone()),
            Some(format!("blocks[{index}].text")),
        );
    }
}

fn first_contrast_label(sentence: &str) -> Option<&'static str> {
    for &(first, second, label) in CONTRAST_PAIRS {
        if pair_within_gap(sentence, first, second) {
            return Some(label);
        }
    }
    None
}

fn pair_within_gap(text: &str, first: &str, second: &str) -> bool {
    let mut search = 0usize;
    while let Some(rest) = text.get(search..) {
        let Some(found) = rest.find(first) else {
            return false;
        };
        let first_start = search + found;
        let first_end = first_start + first.len();
        let Some(after) = text.get(first_end..) else {
            return false;
        };
        if let Some(second_offset) = after.find(second)
            && let Some(gap_text) = after.get(..second_offset)
            && (1..=45).contains(&gap_text.chars().count())
        {
            return true;
        }
        search = first_start + first.len().max(1);
    }
    false
}

fn strong_hedge_hits(text: &str) -> BTreeSet<&'static str> {
    STRONG_HEDGES
        .iter()
        .copied()
        .filter(|phrase| text.contains(*phrase))
        .collect()
}

fn light_hedge_hits(text: &str) -> BTreeSet<&'static str> {
    let mut hits = BTreeSet::new();
    if text.contains("诚然如此") {
        hits.insert("诚然如此");
    } else if text.contains("诚然") {
        hits.insert("诚然");
    }
    for phrase in LIGHT_HEDGES {
        if matches!(*phrase, "诚然" | "诚然如此") {
            continue;
        }
        if text.contains(*phrase) {
            hits.insert(*phrase);
        }
    }
    hits
}
