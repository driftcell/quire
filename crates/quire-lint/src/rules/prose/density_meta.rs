use super::*;

pub(super) fn check_wall_of_text(context: &LintContext<'_>, engine: &mut Engine) {
    let document = context.document;
    for (index, block) in document.blocks.iter().enumerate() {
        let BlockKind::Paragraph { .. } = &block.body else {
            continue;
        };
        let Some(precomputed) = context.paragraph(index) else {
            continue;
        };
        if precomputed.has_list_structure {
            continue;
        }
        let char_len = precomputed.wall_chars;
        let sentence_count = precomputed.wall_sentences;
        if char_len < 360 && (char_len < 300 || sentence_count < 6) {
            continue;
        }
        engine.report(
            RuleId::WallOfTextParagraph,
            format!(
                "paragraph `{}` is a {char_len}-char, {sentence_count}-sentence wall of text without line breaks or list structure",
                block.id
            ),
            "In this Paragraph via apply_operations, give each paragraph one main job and split independent findings, hypotheses and recommendations into focused paragraphs or a list; remove repeated visual data before considering a new Table; fix this together with QR412/QR413 in one apply_operations batch when they hit the same block.",
            Some(block.id.clone()),
            Some(format!("blocks[{index}].text")),
        );
    }
}

pub(super) fn check_dense_prose_section(context: &LintContext<'_>, engine: &mut Engine) {
    let document = context.document;
    let mut section_visual_totals = vec![0usize; document.blocks.len()];
    let mut counted_section = None;
    let mut visual_total = 0usize;
    for (index, block) in document.blocks.iter().enumerate() {
        match &block.body {
            BlockKind::Heading { level, .. } if *level == 1 || *level == 2 => {
                if let Some(start) = counted_section.replace(index)
                    && let Some(total) = section_visual_totals.get_mut(start)
                {
                    *total = visual_total;
                }
                visual_total = 0;
            }
            BlockKind::Table(_) | BlockKind::Echarts(_) | BlockKind::CustomHtml(_)
                if counted_section.is_some() =>
            {
                visual_total = visual_total.saturating_add(1);
            }
            _ => {}
        }
    }
    if let Some(start) = counted_section
        && let Some(total) = section_visual_totals.get_mut(start)
    {
        *total = visual_total;
    }

    let mut parent_h1_exempt = false;
    let mut exempt = false;
    let mut section_visual_count = 0usize;
    let mut prose_blocks = 0usize;
    let mut prose_chars = 0usize;
    let mut reported = false;
    for (index, block) in document.blocks.iter().enumerate() {
        match &block.body {
            BlockKind::Heading { level, text } if *level == 1 || *level == 2 => {
                prose_blocks = 0;
                prose_chars = 0;
                reported = false;
                if *level == 1 {
                    parent_h1_exempt = action_or_closing_section_exempt(text);
                }
                section_visual_count = section_visual_totals.get(index).copied().unwrap_or(0);
                exempt = parent_h1_exempt
                    || dense_prose_section_exempt(text)
                    || section_visual_count >= 2;
            }
            BlockKind::Table(_) | BlockKind::Echarts(_) | BlockKind::CustomHtml(_) => {
                prose_blocks = 0;
                prose_chars = 0;
                reported = false;
            }
            BlockKind::Paragraph { .. } => {
                if !exempt {
                    prose_blocks = prose_blocks.saturating_add(1);
                    prose_chars = prose_chars
                        .saturating_add(context.paragraph(index).map_or(0, |view| view.wall_chars));
                    let max_prose_chars = if section_visual_count >= 1 {
                        1550
                    } else {
                        1500
                    };
                    if !reported && prose_blocks >= 4 && prose_chars >= max_prose_chars {
                        reported = true;
                        engine.report(
                            RuleId::DenseProseRunNeedsVisual,
                            format!(
                                "block `{}` completes a {prose_blocks}-block ({prose_chars}-char) prose run with no Table or Echarts chart",
                                block.id
                            ),
                            "In this section via apply_operations, first remove repeated visual data and split multi-idea prose. If a comparison, trend or structured recommendation still needs a clearer home, extract it into a Table or ECharts chart and trim the prose to new analytical takeaways. Never insert a duplicate Table/ECharts/CustomHtml block merely to reset this prose run or suppress the warning.",
                            Some(block.id.clone()),
                            Some(format!("blocks[{index}].text")),
                        );
                    }
                }
            }
            BlockKind::Quote { text } if !text.trim().is_empty() => {
                if !exempt {
                    prose_blocks = prose_blocks.saturating_add(1);
                    prose_chars = prose_chars
                        .saturating_add(context.paragraph(index).map_or(0, |view| view.wall_chars));
                    let max_prose_chars = if section_visual_count >= 1 {
                        1550
                    } else {
                        1500
                    };
                    if !reported && prose_blocks >= 4 && prose_chars >= max_prose_chars {
                        reported = true;
                        engine.report(
                            RuleId::DenseProseRunNeedsVisual,
                            format!(
                                "block `{}` completes a {prose_blocks}-block ({prose_chars}-char) prose run with no Table or Echarts chart",
                                block.id
                            ),
                            "In this section via apply_operations, first remove repeated visual data and split multi-idea prose. If a comparison, trend or structured recommendation still needs a clearer home, extract it into a Table or ECharts chart and trim the prose to new analytical takeaways. Never insert a duplicate Table/ECharts/CustomHtml block merely to reset this prose run or suppress the warning.",
                            Some(block.id.clone()),
                            Some(format!("blocks[{index}].text")),
                        );
                    }
                }
            }
            BlockKind::Heading { .. } | BlockKind::Quote { .. } => {}
        }
    }
}

pub(super) fn check_editorial_meta_labels(context: &LintContext<'_>, engine: &mut Engine) {
    let document = context.document;
    for (index, block) in document.blocks.iter().enumerate() {
        let mut label = None;
        let mut path = format!("blocks[{index}].text");
        match &block.body {
            BlockKind::Paragraph { .. } => {
                label = context
                    .paragraph(index)
                    .and_then(|view| editorial_meta_label(&view.masked));
            }
            BlockKind::Heading { text, .. } => {
                label = editorial_meta_label(&mask_text(text));
            }
            BlockKind::Table(table) => {
                path = format!("blocks[{index}].title");
                label = table
                    .title
                    .as_deref()
                    .and_then(|title| editorial_meta_label(&mask_text(title)))
                    .or_else(|| {
                        path = format!("blocks[{index}].columns");
                        column_meta_label(&table.columns)
                    });
            }
            BlockKind::Echarts(chart) => {
                path = format!("blocks[{index}].title");
                label = chart
                    .title
                    .as_deref()
                    .and_then(|title| editorial_meta_label(&mask_text(title)));
            }
            BlockKind::CustomHtml(html) => {
                path = format!("blocks[{index}].title");
                label = html
                    .title
                    .as_deref()
                    .and_then(|title| editorial_meta_label(&mask_text(title)));
            }
            BlockKind::Quote { .. } => {}
        }
        let Some(label) = label else {
            continue;
        };
        engine.report(
            RuleId::EditorialMetaLabel,
            format!(
                "block `{}` exposes the editorial meta-label `{label}` to report readers",
                block.id
            ),
            "Rewrite the statement in direct publication language: state the verified fact, calibrated inference or recommendation itself instead of attaching `核实值`, `属推断`, `本报告判断` or `本报告建议`. Move genuinely necessary scenario assumptions into a concise visual note; do not merely swap synonyms.",
            Some(block.id.clone()),
            Some(path),
        );
    }
}

fn column_meta_label(columns: &[ColumnNode]) -> Option<&'static str> {
    for column in columns {
        if let Some(label) = editorial_meta_label(&mask_text(&column.label)) {
            return Some(label);
        }
        if let Some(label) = column_meta_label(&column.children) {
            return Some(label);
        }
    }
    None
}

fn editorial_meta_label(text: &str) -> Option<&'static str> {
    for phrase in [
        "本报告建议",
        "本报告判断",
        "本报告不作逐条引证",
        "本报告不将其计入既有事实",
        "本节只确认",
        "本章只确认",
        "在行动路径章统一处理",
        "在后续章节统一处理",
        "需要在申报口径上收敛的一处细节",
        "这一位置决定了它在申报中的用法",
        "是基于学科名称的推断",
    ] {
        if text.contains(phrase) {
            return Some(phrase);
        }
    }
    for (open, close) in [('（', '）'), ('(', ')')] {
        for span in text.split(open).skip(1) {
            let Some((inside, _)) = span.split_once(close) else {
                continue;
            };
            if inside.chars().count() <= 60 {
                for phrase in ["核实值", "属推断"] {
                    if inside.contains(phrase) {
                        return Some(phrase);
                    }
                }
            }
        }
    }
    None
}
