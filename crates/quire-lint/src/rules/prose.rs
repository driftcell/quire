//! Static AI-slop and low-evidence prose checks for paragraph blocks.

use std::{collections::BTreeSet, sync::LazyLock};

use quire_protocol::{BlockKind, ReportDocument};
use regex::Regex;

use super::{evidence::has_quantitative_evidence, structure::dense_prose_section_exempt};
use crate::{engine::Engine, registry::RuleId};

const CLICHE_TRANSITIONS: &[&str] = &[
    "综上所述",
    "总而言之",
    "概而言之",
    "简而言之",
    "换言之",
    "归根结底",
    "毫无疑问",
    "显而易见",
    "不难发现",
    "众所周知",
    "毋庸置疑",
    "由此可见",
    "需要指出的是",
    "不可忽视的是",
    "值得关注的是",
    "在此背景下",
    "面对这一形势",
];

const REPEATED_TRANSITIONS: &[&str] = &["与此同时", "值得注意的是"];
const ENUMERATION_MIN_ITEM_CHARS: usize = 8;
const ENUMERATION_AVG_ITEM_CHARS: usize = 18;

const PROCESS_AND_DISCLAIMER_CLICHES: &[&str] = &[
    "受篇幅所限",
    "以上判断仅供参考",
    "仅供参考",
    "本报告认为",
    "经联网检索",
    "不妥之处敬请批评指正",
    "由于水平有限",
    "值得进一步观察",
    "但不构成简单对应",
    "须合并解读",
];

static CHAPTER_GUIDE_RE: LazyLock<Option<Regex>> = LazyLock::new(|| {
    Regex::new(
        r"(?:本章|本节|下文|后文|接下来)[^。；！？\n]{0,24}(?:先看|(?:从|以|围绕|按照|通过)[^。；！？\n]{0,12}[一二两三四五六七八九十\d]+个?(?:维度|方面|观测面|角度|层面|视角|步骤|部分))|(?:观察|分析|研判|审视)?(?:角度|维度|观测面|层面|视角)有[一二两三四五六七八九十\d]+",
    )
    .ok()
});

const EXTREME_JARGON_CLICHES: &[&str] = &[
    "底层逻辑",
    "范式跃迁",
    "生态位",
    "护城河",
    "飞轮",
    "颗粒度",
    "深度赋能",
    "辉煌成就",
    "无比强劲",
    "质的飞跃",
];

const EVIDENCE_DEPENDENT_EVALUATIONS: &[&str] =
    &["提质增效", "显著提升", "稳中有进", "总体平稳", "令人瞩目"];

const ENGLISH_CLICHES: &[&str] = &[
    "in conclusion",
    "it is worth noting",
    "it is important to note",
    "needless to say",
    "in summary",
    "that being said",
    "at the end of the day",
];

const EMPTY_JARGON: &[&str] = &[
    "赋能",
    "抓手",
    "矩阵",
    "打法",
    "闭环",
    "生态",
    "赛道",
    "破局",
    "筑牢",
    "夯实",
    "锚定",
    "绘就",
    "彰显",
    "凸显",
    "助推",
    "深耕",
    "厚植",
    "跃升",
    "蝶变",
    "弯道超车",
    "双轮驱动",
    "组合拳",
    "协同发力",
    "同频共振",
    "多措并举",
    "多维发力",
    "纵深推进",
    "精准施策",
    "跨越式发展",
];

const ECO_SUFFIXES: &[&str] = &[
    "环境",
    "系统",
    "学",
    "保护",
    "修复",
    "治理",
    "文明",
    "补偿",
    "监测",
    "安全",
    "红线",
    "容量",
    "承载力",
    "脆弱性",
    "敏感性",
    "多样性",
    "平衡",
    "格局",
    "空间",
    "功能",
    "服务",
    "产品",
    "旅游",
    "农业",
    "林业",
    "牧场",
    "公园",
    "廊道",
    "网络",
    "足迹",
    "效率",
    "质量",
    "状况",
    "风险",
    "灾害",
    "工程",
    "技术",
    "模型",
    "遥感",
    "地质",
    "气候",
    "水文",
    "碳汇",
    "碳源",
    "价值",
    "资产",
    "资本",
    "建设",
    "发展",
    "恢复",
    "维护",
    "评估",
    "规划",
    "管理",
    "调查",
    "观测",
    "研究",
    "教育",
    "康养",
    "宜居",
    "屏障",
    "基底",
    "本底",
];

const CONTRAST_PAIRS: &[(&str, &str, &str)] = &[
    ("不是说", "而是说", "不是说…而是说…"),
    ("不是说", "而是", "不是说…而是…"),
    ("不再是", "而是", "不再是…而是…"),
    ("不是", "而是", "不是…而是…"),
    ("并非", "而是", "并非…而是…"),
    ("与其说", "不如说", "与其说…不如说…"),
    ("与其", "不如", "与其…不如…"),
    ("不仅在于", "更在于", "不仅在于…更在于…"),
    ("不仅是", "更是", "不仅是…更是…"),
];

const STRONG_HEDGES: &[&str] = &[
    "严格来说",
    "客观而言",
    "坦率地说",
    "不可否认",
    "从某种意义上讲",
    "从某种意义上说",
    "某种程度上",
    "在一定程度上",
    "需要强调的是",
    "必须指出的是",
    "需要说明的是",
];

const LIGHT_HEDGES: &[&str] = &[
    "总体来看",
    "整体而言",
    "大体上",
    "相对而言",
    "诚然",
    "诚然如此",
    "尽管如此",
    "换个角度看",
];

const ECO_PREFIXES: &[&str] = &[
    "海洋",
    "陆地",
    "淡水",
    "森林",
    "草原",
    "湿地",
    "荒漠",
    "农田",
    "城市",
    "流域",
    "区域",
    "全球",
    "自然",
    "水",
    "土壤",
    "微生物",
    "植物",
    "动物",
    "农业",
    "林业",
    "草地",
    "湖泊",
    "河流",
    "山地",
    "高原",
    "极地",
    "深海",
    "近海",
    "海岸",
    "红树林",
    "珊瑚礁",
    "生物",
    "保护",
    "修复",
    "治理",
    "建设",
    "改善",
    "恢复",
    "维护",
    "监测",
    "评估",
    "规划",
    "管理",
    "补偿",
    "绿色",
    "低碳",
    "循环",
];

pub(crate) fn check(document: &ReportDocument, engine: &mut Engine<'_>) {
    check_cliche_density(document, engine);
    check_low_evidence(document, engine);
    check_template_parallel(document, engine);
    check_contrast_and_hedging(document, engine);
    check_wall_of_text(document, engine);
    check_dense_prose_section(document, engine);
}

fn check_cliche_density(document: &ReportDocument, engine: &mut Engine<'_>) {
    let mut previous_transition = None;
    for (index, block) in document.blocks.iter().enumerate() {
        let BlockKind::Paragraph { text } = &block.body else {
            previous_transition = None;
            continue;
        };
        let masked = mask_text(text);
        let mut hits = BTreeSet::new();
        for phrase in CLICHE_TRANSITIONS
            .iter()
            .chain(PROCESS_AND_DISCLAIMER_CLICHES)
            .chain(EXTREME_JARGON_CLICHES)
        {
            if masked.contains(*phrase) {
                hits.insert(*phrase);
            }
        }
        if CHAPTER_GUIDE_RE
            .as_ref()
            .is_some_and(|pattern| pattern.is_match(&masked))
        {
            hits.insert("chapter-preview narration");
        }
        for phrase in ENGLISH_CLICHES {
            if contains_ascii_phrase(&masked, phrase) {
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

        let mut jargon_hits = Vec::new();
        for phrase in EMPTY_JARGON {
            if contains_jargon(&masked, phrase) {
                jargon_hits.push(*phrase);
            }
        }
        if jargon_hits.len() >= 2 {
            hits.extend(jargon_hits);
        }
        if masked.matches("组合拳").count() >= 2 {
            hits.insert("组合拳");
        }

        let leading_transition = leading_repeated_transition(&masked);
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

fn leading_repeated_transition(text: &str) -> Option<&'static str> {
    let trimmed = text.trim_start();
    REPEATED_TRANSITIONS
        .iter()
        .copied()
        .find(|phrase| trimmed.starts_with(phrase))
}

fn check_low_evidence(document: &ReportDocument, engine: &mut Engine<'_>) {
    for (index, block) in document.blocks.iter().enumerate() {
        let BlockKind::Paragraph { text } = &block.body else {
            continue;
        };
        if text.chars().any(|ch| ch.is_ascii_digit())
            || has_inline_citation(text)
            || !block.source_ids.is_empty()
            || has_book_title(text)
        {
            continue;
        }
        let masked = mask_text(text);
        let length = masked_plain_char_count(&masked);
        if length < 120 {
            continue;
        }
        engine.report(
            RuleId::LowEvidenceProse,
            format!(
                "block `{}` has {length} prose characters but no number, year, metric or citation",
                block.id
            ),
            "In this Paragraph via apply_operations, add a real number or `[^source_id]` already retrieved by tools, or cut the unsupported prose; never invent figures or citations to satisfy the check.",
            Some(block.id.clone()),
            Some(format!("blocks[{index}].text")),
        );
    }
}

fn check_template_parallel(document: &ReportDocument, engine: &mut Engine<'_>) {
    let mut contrast_occurrences = 0usize;
    for (index, block) in document.blocks.iter().enumerate() {
        let BlockKind::Paragraph { text } = &block.body else {
            continue;
        };
        let masked = mask_text(text);
        let has_citation_evidence = has_inline_citation(text) || !block.source_ids.is_empty();
        let mut reasons = Vec::new();

        if contains_in_order(&masked, &["首先", "其次", "最后"])
            || contains_in_order(&masked, &["首先", "再次", "最后"])
        {
            reasons.push("机械三段式连接词");
        }
        if !has_list_structure(text) {
            if contains_punctuated_sequence(&masked, &["其一", "其二", "其三"]) {
                reasons.push("机械“其一/其二/其三”枚举");
            }
            if contains_punctuated_sequence(&masked, &["第一", "第二", "第三"]) {
                reasons.push("机械“第一/第二/第三”枚举");
            }
        }
        if !has_list_structure(text) {
            if let Some(items) =
                ordered_enumeration_items(&masked, &["一是", "二是", "三是", "四是"])
            {
                if !substantive_enumeration(&masked, &items, has_citation_evidence) {
                    reasons.push("机械“一是/二是/三是/四是”枚举");
                }
            } else if let Some(items) =
                ordered_enumeration_items(&masked, &["一是", "二是", "三是"])
                && !substantive_enumeration(&masked, &items, has_citation_evidence)
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

fn check_contrast_and_hedging(document: &ReportDocument, engine: &mut Engine<'_>) {
    let mut contrast_sentences_in_document = 0usize;
    for (index, block) in document.blocks.iter().enumerate() {
        let BlockKind::Paragraph { text } = &block.body else {
            continue;
        };
        let masked = mask_text(text);
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

        let strong_hedges = strong_hedge_hits(&masked);
        let light_hedges = light_hedge_hits(&masked);
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

fn check_wall_of_text(document: &ReportDocument, engine: &mut Engine<'_>) {
    for (index, block) in document.blocks.iter().enumerate() {
        let BlockKind::Paragraph { text } = &block.body else {
            continue;
        };
        if has_list_structure(text) {
            continue;
        }
        let char_len = wall_of_text_char_count(text);
        if char_len < 450 {
            continue;
        }
        engine.report(
            RuleId::WallOfTextParagraph,
            format!(
                "paragraph `{}` is a {char_len}-char wall of text without line breaks or list structure",
                block.id
            ),
            "In this Paragraph via apply_operations, split it into 2-3 focused paragraphs or convert enumerations to a bullet list/Table; fix this together with QR412/QR413 in one apply_operations batch when they hit the same block.",
            Some(block.id.clone()),
            Some(format!("blocks[{index}].text")),
        );
    }
}

fn check_dense_prose_section(document: &ReportDocument, engine: &mut Engine<'_>) {
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

    let mut exempt = false;
    let mut prose_blocks = 0usize;
    let mut prose_chars = 0usize;
    let mut reported = false;
    for (index, block) in document.blocks.iter().enumerate() {
        match &block.body {
            BlockKind::Heading { level, text } if *level == 1 || *level == 2 => {
                prose_blocks = 0;
                prose_chars = 0;
                reported = false;
                exempt = dense_prose_section_exempt(text)
                    || section_visual_totals.get(index).copied().unwrap_or(0) >= 2;
            }
            BlockKind::Table(_) | BlockKind::Echarts(_) | BlockKind::CustomHtml(_) => {
                prose_blocks = 0;
                prose_chars = 0;
                reported = false;
            }
            BlockKind::Paragraph { text } => {
                if !exempt {
                    prose_blocks = prose_blocks.saturating_add(1);
                    prose_chars = prose_chars.saturating_add(wall_of_text_char_count(text));
                    if !reported && prose_blocks >= 4 && prose_chars >= 1000 {
                        reported = true;
                        engine.report(
                            RuleId::DenseProseRunNeedsVisual,
                            format!(
                                "block `{}` completes a {prose_blocks}-block ({prose_chars}-char) prose run with no Table or Echarts chart",
                                block.id
                            ),
                            "In this section via apply_operations, extract multi-entity comparisons or per-school recommendations into a Table or Echarts chart (e.g. stacked bar for talent/metrics or summary action table), and trim the prose to analytical takeaways; never delete real facts merely to drop below the char limit.",
                            Some(block.id.clone()),
                            Some(format!("blocks[{index}].text")),
                        );
                    }
                }
            }
            BlockKind::Quote { text } if !text.trim().is_empty() => {
                if !exempt {
                    prose_blocks = prose_blocks.saturating_add(1);
                    prose_chars = prose_chars.saturating_add(wall_of_text_char_count(text));
                    if !reported && prose_blocks >= 4 && prose_chars >= 1000 {
                        reported = true;
                        engine.report(
                            RuleId::DenseProseRunNeedsVisual,
                            format!(
                                "block `{}` completes a {prose_blocks}-block ({prose_chars}-char) prose run with no Table or Echarts chart",
                                block.id
                            ),
                            "In this section via apply_operations, extract multi-entity comparisons or per-school recommendations into a Table or Echarts chart (e.g. stacked bar for talent/metrics or summary action table), and trim the prose to analytical takeaways; never delete real facts merely to drop below the char limit.",
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

fn has_list_structure(text: &str) -> bool {
    if !text.contains('\n') {
        return false;
    }
    text.lines().any(|line| {
        let trimmed = line.trim_start();
        trimmed.starts_with("- ")
            || trimmed.starts_with("* ")
            || trimmed.starts_with("+ ")
            || numbered_list_line(trimmed)
    })
}

fn numbered_list_line(line: &str) -> bool {
    let mut digit_end = 0usize;
    for (index, ch) in line.char_indices() {
        if ch.is_ascii_digit() {
            digit_end = index + ch.len_utf8();
        } else {
            break;
        }
    }
    if digit_end == 0 {
        return false;
    }
    line.get(digit_end..)
        .is_some_and(|rest| rest.starts_with(". "))
}

fn wall_of_text_char_count(text: &str) -> usize {
    strip_wall_markup(text)
        .chars()
        .filter(|ch| !ch.is_whitespace() && !is_markdown_symbol(*ch))
        .count()
}

fn strip_wall_markup(text: &str) -> String {
    let mut output = String::with_capacity(text.len());
    let mut cursor = 0usize;
    while cursor < text.len() {
        let Some(rest) = text.get(cursor..) else {
            break;
        };
        let Some(ch) = rest.chars().next() else {
            break;
        };
        if ch == '`' {
            cursor += ch.len_utf8();
            continue;
        }
        if rest.starts_with("[^")
            && let Some(end) = bracket_close_end(text, cursor)
        {
            cursor = end;
            continue;
        }
        if ch == '['
            && let Some((label_end, target_end)) = markdown_link_end(text, cursor)
        {
            if let Some(label) = text.get(cursor + 1..label_end) {
                output.push_str(label);
            }
            cursor = target_end;
            continue;
        }
        output.push(ch);
        cursor += ch.len_utf8();
    }
    output
}

fn markdown_link_end(text: &str, start: usize) -> Option<(usize, usize)> {
    let label_end = matching_bracket_end(text, start)?;
    let after = text.get(label_end + 1..)?;
    if !after.starts_with('(') {
        return None;
    }
    let target_end = parenthesis_end(text, label_end + 1)?;
    Some((label_end, target_end))
}

fn matching_bracket_end(text: &str, start: usize) -> Option<usize> {
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

pub(super) fn mask_text(text: &str) -> String {
    let mut output = String::with_capacity(text.len());
    let mut cursor = 0usize;
    while cursor < text.len() {
        let Some(rest) = text.get(cursor..) else {
            break;
        };
        let Some(ch) = rest.chars().next() else {
            break;
        };
        let span_end = match ch {
            '`' => code_span_end(text, cursor),
            '[' if rest.starts_with("[^") => bracket_close_end(text, cursor),
            '(' if text
                .get(..cursor)
                .is_some_and(|prefix| prefix.ends_with(']')) =>
            {
                parenthesis_end(text, cursor)
            }
            '《' => delimited_end(text, cursor, '》'),
            '“' => delimited_end(text, cursor, '”'),
            '"' => delimited_end(text, cursor, '"'),
            '「' => delimited_end(text, cursor, '」'),
            _ => None,
        };
        if let Some(end) = span_end {
            if let Some(span) = text.get(cursor..end) {
                push_spaces(&mut output, span);
            }
            cursor = end;
        } else {
            output.push(ch);
            cursor += ch.len_utf8();
        }
    }
    output
}

fn push_spaces(output: &mut String, text: &str) {
    for _ in text.chars() {
        output.push(' ');
    }
}

fn code_span_end(text: &str, start: usize) -> Option<usize> {
    let tail = text.get(start..)?;
    let run = tail.bytes().take_while(|byte| *byte == b'`').count();
    let content_start = start + run;
    let mut search = content_start;
    while let Some(rest) = text.get(search..) {
        let found = rest.find('`')?;
        let absolute = search + found;
        let run_tail = text.get(absolute..)?;
        let closing_run = run_tail.bytes().take_while(|byte| *byte == b'`').count();
        if closing_run == run {
            return Some(absolute + closing_run);
        }
        search = absolute + closing_run;
    }
    None
}

fn bracket_close_end(text: &str, start: usize) -> Option<usize> {
    let tail = text.get(start + 2..)?;
    let found = tail.find(']')?;
    Some(start + 2 + found + 1)
}

fn parenthesis_end(text: &str, start: usize) -> Option<usize> {
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
            '(' => depth += 1,
            ')' => {
                depth = depth.checked_sub(1)?;
                if depth == 0 {
                    return Some(index + ch.len_utf8());
                }
            }
            _ => {}
        }
        index += ch.len_utf8();
    }
    None
}

fn delimited_end(text: &str, start: usize, close: char) -> Option<usize> {
    let open = text.get(start..)?.chars().next()?;
    let content_start = start + open.len_utf8();
    let tail = text.get(content_start..)?;
    let found = tail.find(close)?;
    Some(content_start + found + close.len_utf8())
}

fn contains_ascii_phrase(text: &str, phrase: &str) -> bool {
    let lowered = text.to_ascii_lowercase();
    let mut search = 0usize;
    while let Some(rest) = lowered.get(search..) {
        let Some(found) = rest.find(phrase) else {
            return false;
        };
        let absolute = search + found;
        let before_ok = lowered
            .get(..absolute)
            .and_then(|prefix| prefix.chars().next_back())
            .is_none_or(|ch| !is_ascii_word_char(ch));
        let after = absolute + phrase.len();
        let after_ok = lowered
            .get(after..)
            .and_then(|suffix| suffix.chars().next())
            .is_none_or(|ch| !is_ascii_word_char(ch));
        if before_ok && after_ok {
            return true;
        }
        search = absolute + phrase.len().max(1);
    }
    false
}

fn is_ascii_word_char(ch: char) -> bool {
    ch.is_ascii_alphanumeric() || ch == '_'
}

fn contains_jargon(text: &str, phrase: &str) -> bool {
    if phrase == "生态" {
        return contains_unguarded_eco(text);
    }
    text.contains(phrase)
}

fn contains_unguarded_eco(text: &str) -> bool {
    let mut search = 0usize;
    while let Some(rest) = text.get(search..) {
        let Some(found) = rest.find("生态") else {
            return false;
        };
        let absolute = search + found;
        if !is_eco_compound(text, absolute) {
            return true;
        }
        search = absolute + "生态".len();
    }
    false
}

fn is_eco_compound(text: &str, index: usize) -> bool {
    let before = text.get(..index).unwrap_or("");
    let after = text.get(index + "生态".len()..).unwrap_or("");
    ECO_PREFIXES.iter().any(|prefix| before.ends_with(prefix))
        || ECO_SUFFIXES.iter().any(|suffix| after.starts_with(suffix))
}

fn has_inline_citation(text: &str) -> bool {
    let mut search = 0usize;
    while let Some(rest) = text.get(search..) {
        let Some(found) = rest.find("[^") else {
            return false;
        };
        let absolute = search + found + 2;
        if text.get(absolute..).is_some_and(|tail| tail.contains(']')) {
            return true;
        }
        search = absolute;
    }
    false
}

fn has_book_title(text: &str) -> bool {
    let mut search = 0usize;
    while let Some(rest) = text.get(search..) {
        let Some(found) = rest.find('《') else {
            return false;
        };
        let absolute = search + found + '《'.len_utf8();
        if text.get(absolute..).is_some_and(|tail| tail.contains('》')) {
            return true;
        }
        search = absolute;
    }
    false
}

fn masked_plain_char_count(text: &str) -> usize {
    text.chars()
        .filter(|ch| !ch.is_whitespace() && !is_markdown_symbol(*ch))
        .count()
}

fn is_markdown_symbol(ch: char) -> bool {
    matches!(
        ch,
        '*' | '_'
            | '~'
            | '`'
            | '#'
            | '>'
            | '-'
            | '+'
            | '='
            | '['
            | ']'
            | '('
            | ')'
            | '!'
            | '|'
            | '\\'
    )
}

fn contains_punctuated_sequence(text: &str, items: &[&str]) -> bool {
    let mut search = 0usize;
    for item in items {
        let mut matched = false;
        while let Some(rest) = text.get(search..) {
            let Some(found) = rest.find(item) else {
                return false;
            };
            let absolute = search + found;
            let after = absolute + item.len();
            let Some(next) = text.get(after..).and_then(|tail| tail.chars().next()) else {
                return false;
            };
            if matches!(next, '，' | '、' | '：' | ',' | ':') {
                search = after + next.len_utf8();
                matched = true;
                break;
            }
            search = after;
        }
        if !matched {
            return false;
        }
    }
    true
}

fn contains_in_order(text: &str, items: &[&str]) -> bool {
    let mut search = 0usize;
    for item in items {
        let Some(rest) = text.get(search..) else {
            return false;
        };
        let Some(found) = rest.find(item) else {
            return false;
        };
        search += found + item.len();
    }
    true
}

fn bold_list_count(text: &str) -> usize {
    text.lines()
        .filter(|line| list_bold_prefix_end(line).is_some())
        .count()
}

fn list_bold_prefix_end(line: &str) -> Option<usize> {
    let trimmed = line.trim_start();
    for prefix in ["- **", "* **"] {
        if trimmed.starts_with(prefix) {
            return Some(prefix.len());
        }
    }
    let mut digit_end = 0usize;
    for (index, ch) in trimmed.char_indices() {
        if ch.is_ascii_digit() {
            digit_end = index + ch.len_utf8();
        } else {
            break;
        }
    }
    if digit_end == 0 {
        return None;
    }
    trimmed
        .get(digit_end..)
        .and_then(|rest| rest.starts_with(". **").then_some(digit_end + ". **".len()))
}

fn has_digit_outside_list_markers(text: &str) -> bool {
    for line in text.lines() {
        let trimmed = line.trim_start();
        let rest = match list_bold_prefix_end(line) {
            Some(prefix_end) => trimmed.get(prefix_end..),
            None => Some(trimmed),
        };
        if rest.is_some_and(|value| value.chars().any(|ch| ch.is_ascii_digit())) {
            return true;
        }
    }
    false
}
