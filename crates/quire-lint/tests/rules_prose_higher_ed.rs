#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::indexing_slicing,
    clippy::panic
)]

pub mod common;
use common::*;

#[test]
fn qr412_detects_strong_ai_cliches() {
    let mut document = doc();
    document
        .blocks
        .push(paragraph("summary", "综上所述，项目进展顺利。"));
    let report = lint_document(&document);
    let diagnostic = finding(&report, "QR412");
    assert_eq!(diagnostic.severity, Severity::Warning);
    assert!(diagnostic.suppressible);
    assert!(
        diagnostic.message.contains("综上所述"),
        "{}",
        diagnostic.message
    );
    assert!(
        diagnostic.hint.contains("effect size"),
        "{}",
        diagnostic.hint
    );
    assert!(
        diagnostic
            .hint
            .contains("quote exact policy or source terms in \"\" if needed."),
        "{}",
        diagnostic.hint
    );
}

#[test]
fn qr412_detects_process_disclaimer_and_extreme_cliches() {
    for (id, text, phrase) in [
        (
            "process-limit",
            "受篇幅所限，本文只讨论核心指标。",
            "受篇幅所限",
        ),
        ("online-search", "经联网检索，相关数据如下。", "经联网检索"),
        ("bottom-logic", "底层逻辑在于资源配置。", "底层逻辑"),
        ("granularity", "提升数据颗粒度。", "颗粒度"),
        (
            "chapter-from",
            "本章从两个维度审读学科发展。",
            "chapter-preview narration",
        ),
        (
            "below-first",
            "下文先看学科档位，再判断人才结构。",
            "chapter-preview narration",
        ),
        (
            "two-angles",
            "观察角度有二：规模与质量。",
            "chapter-preview narration",
        ),
        (
            "three-angles",
            "观察角度有三：规模、质量与结构。",
            "chapter-preview narration",
        ),
        (
            "three-views",
            "本章以三个观测面展开分析。",
            "chapter-preview narration",
        ),
        (
            "four-levels",
            "本节围绕四个层面依次展开。",
            "chapter-preview narration",
        ),
        (
            "numbered-guide",
            "接下来从2个步骤审视建设路径。",
            "chapter-preview narration",
        ),
        ("joint-reading", "二者须合并解读，不能割裂。", "须合并解读"),
    ] {
        let mut document = doc();
        document.blocks.push(paragraph(id, text));
        let report = lint_document(&document);
        let diagnostic = finding(&report, "QR412");
        assert!(
            diagnostic.message.contains(phrase),
            "{}",
            diagnostic.message
        );
        assert!(diagnostic.hint.contains("chapter-preview narration"));
    }
}

#[test]
fn qr412_counts_new_b_class_words_by_distinct_density() {
    let mut single = doc();
    single
        .blocks
        .push(paragraph("single", "锚定目标需要长期投入。"));
    assert!(!has_code(&lint_document(&single), "QR412"));

    let mut pair = doc();
    pair.blocks.push(paragraph("pair", "锚定目标、夯实基础。"));
    assert!(has_code(&lint_document(&pair), "QR412"));

    let mut tactics_single = doc();
    tactics_single
        .blocks
        .push(paragraph("tactics-single", "市场打法需要调整。"));
    assert!(
        !has_code(&lint_document(&tactics_single), "QR412"),
        "single B-class word must remain below the density threshold"
    );

    let mut tactics_pair = doc();
    tactics_pair
        .blocks
        .push(paragraph("tactics-pair", "市场打法与赋能协同。"));
    assert!(has_code(&lint_document(&tactics_pair), "QR412"));
}

#[test]
fn qr412_aligns_formal_terms_and_evaluations_with_evidence() {
    for (id, text) in [
        (
            "policy-terms",
            "学校坚持高质量发展、内涵式发展，系统推进“双一流”建设。",
        ),
        (
            "supported-evaluation",
            "国家级项目由158项增至214项，同比增长35.4%，人才培养质量显著提升。",
        ),
        ("single-combination", "学校以组合拳推进跨学科课程建设。"),
        ("single-transition", "与此同时，学校完善质量保障。"),
        ("single-noting", "值得注意的是，数据仍有缺口。"),
    ] {
        let mut document = doc();
        document.blocks.push(paragraph(id, text));
        assert!(
            !has_code(&lint_document(&document), "QR412"),
            "formal or supported prose must remain clean: {text}"
        );
    }

    let mut unsupported = doc();
    unsupported.blocks.push(paragraph(
        "unsupported-evaluation",
        "2025年，学校办学水平显著提升，发展态势稳中有进。",
    ));
    let report = lint_document(&unsupported);
    let diagnostic = finding(&report, "QR412");
    assert!(diagnostic.message.contains("显著提升"));
    assert!(diagnostic.message.contains("稳中有进"));

    let mut repeated_combination = doc();
    repeated_combination.blocks.push(paragraph(
        "repeated-combination",
        "第一项举措打出组合拳，第二项举措再次打出组合拳。",
    ));
    assert!(has_code(&lint_document(&repeated_combination), "QR412"));
}

#[test]
fn qr412_flags_only_repeated_mechanical_transitions() {
    let mut repeated = doc();
    repeated
        .blocks
        .push(paragraph("first", "与此同时，学校完善课程建设。"));
    repeated
        .blocks
        .push(paragraph("second", "与此同时，学校健全质量保障。"));
    let report = lint_document(&repeated);
    let hits: Vec<_> = report
        .diagnostics
        .iter()
        .filter(|diagnostic| diagnostic.code == "QR412")
        .collect();
    assert_eq!(hits.len(), 1, "{:?}", report.diagnostics);
    assert_eq!(hits[0].block_id.as_ref(), Some(&bid("second")));

    let mut within_paragraph = doc();
    within_paragraph.blocks.push(paragraph(
        "within-paragraph",
        "与此同时，学校完善课程建设；与此同时，学校健全质量保障。",
    ));
    assert!(has_code(&lint_document(&within_paragraph), "QR412"));

    let mut separated = doc();
    separated
        .blocks
        .push(paragraph("before", "值得注意的是，课程建设仍需完善。"));
    separated.blocks.push(heading("separator", 2, "质量保障"));
    separated
        .blocks
        .push(paragraph("after", "值得注意的是，质量保障仍需健全。"));
    assert!(!has_code(&lint_document(&separated), "QR412"));
}

#[test]
fn qr412_allows_policy_terms_and_requires_two_distinct_jargon_words() {
    let mut single = doc();
    single
        .blocks
        .push(paragraph("single", "高质量发展需要长期投入。"));
    assert!(
        !has_code(&lint_document(&single), "QR412"),
        "one jargon word must not cross the density threshold"
    );

    let mut dense = doc();
    dense
        .blocks
        .push(paragraph("dense", "双轮驱动、精准赋能与闭环机制共同推进。"));
    let report = lint_document(&dense);
    let diagnostic = finding(&report, "QR412");
    assert!(diagnostic.message.contains("双轮驱动"));
    assert!(diagnostic.message.contains("赋能"));
    assert!(diagnostic.message.contains("闭环"));
}

#[test]
fn qr412_masks_quoted_proper_nouns_and_skips_quotes() {
    let mut titles = doc();
    titles.blocks.push(paragraph(
        "title",
        "《“双一流”高质量发展意见》提出新的建设任务。",
    ));
    titles.blocks.push(paragraph(
        "quoted",
        "“生态环境保护与海洋生态修复”是长期任务。",
    ));
    titles.blocks.push(paragraph(
        "code",
        "`in conclusion` 与 [链接](https://example.com/in-conclusion) 只是示例。",
    ));
    titles
        .blocks
        .push(paragraph("ascii-quoted", "\"in conclusion\" 是引文。"));
    titles
        .blocks
        .push(paragraph("corner-quoted", "「综上所述」是引文。"));
    titles
        .blocks
        .push(paragraph("quoted-strong", "“受篇幅所限”只是引文。"));
    titles
        .blocks
        .push(paragraph("strong-title", "《受篇幅所限》是文件原文。"));
    titles
        .blocks
        .push(paragraph("development-title", "《高质量发展》是政策文件。"));
    let report = lint_document(&titles);
    assert!(
        !has_code(&report, "QR412"),
        "quoted titles, inline code and link targets must be masked: {:?}",
        report.diagnostics
    );

    let mut quote = doc();
    quote.blocks.push(Block::quote(
        bid("external-quote"),
        "综上所述，双轮驱动与精准赋能形成闭环。",
    ));
    assert!(
        !has_code(&lint_document(&quote), "QR412"),
        "external quotations must not be scanned as authored prose"
    );

    let mut heading_only = doc();
    heading_only
        .blocks
        .push(heading("heading-cliche", 1, "综上所述"));
    assert!(
        !has_code(&lint_document(&heading_only), "QR412"),
        "headings must not be scanned as prose"
    );
}

#[test]
fn qr412_exempts_ecological_compounds() {
    let mut document = doc();
    document.blocks.push(paragraph(
        "ecology",
        "生态环境保护与海洋生态修复是重点，双轮驱动同样重要。",
    ));
    let report = lint_document(&document);
    assert!(
        !has_code(&report, "QR412"),
        "ecological compounds must not count as jargon: {:?}",
        report.diagnostics
    );
}

#[test]
fn qr413_flags_long_prose_without_evidence() {
    let text = "数".repeat(130);
    let mut document = doc();
    document.blocks.push(paragraph("long", text.as_str()));
    let report = lint_document(&document);
    let diagnostic = finding(&report, "QR413");
    assert_eq!(diagnostic.severity, Severity::Info);
    assert!(diagnostic.suppressible);
    assert!(
        diagnostic.hint.contains("already retrieved by tools"),
        "{}",
        diagnostic.hint
    );

    let mut dated = doc();
    let dated_text = format!("{text}2025年");
    dated.blocks.push(paragraph("dated", dated_text.as_str()));
    assert!(!has_code(&lint_document(&dated), "QR413"));

    let mut cited = doc();
    let cited_text = format!("{text}[^src_1]");
    cited.blocks.push(paragraph("cited", cited_text.as_str()));
    assert!(!has_code(&lint_document(&cited), "QR413"));

    let mut titled = doc();
    let titled_text = format!("{text}《政策文件》");
    titled
        .blocks
        .push(paragraph("titled", titled_text.as_str()));
    assert!(!has_code(&lint_document(&titled), "QR413"));
}

#[test]
fn qr414_detects_mechanical_templates() {
    let mut connectives = doc();
    connectives.blocks.push(paragraph(
        "connectives",
        "首先明确目标，其次分解任务，最后跟踪结果。",
    ));
    assert!(has_code(&lint_document(&connectives), "QR414"));

    let mut single_contrast = doc();
    single_contrast.blocks.push(paragraph(
        "single-contrast",
        "一方面看规模，另一方面看结构。",
    ));
    assert!(
        !has_code(&lint_document(&single_contrast), "QR414"),
        "one contrast pair must remain unflagged"
    );

    let mut repeated_contrast = doc();
    for index in 1..=3 {
        let id = format!("contrast-{index}");
        repeated_contrast
            .blocks
            .push(paragraph(id.as_str(), "一方面看规模，另一方面看结构。"));
    }
    let report = lint_document(&repeated_contrast);
    let hits: Vec<_> = report
        .diagnostics
        .iter()
        .filter(|diagnostic| diagnostic.code == "QR414")
        .collect();
    assert_eq!(hits.len(), 1, "{:?}", report.diagnostics);
    assert_eq!(hits[0].block_id.as_ref(), Some(&bid("contrast-3")));

    let mut bullets = doc();
    bullets.blocks.push(paragraph(
        "bullets",
        "- **目标**：明确方向\n- **路径**：分解任务\n- **保障**：跟踪结果",
    ));
    assert!(has_code(&lint_document(&bullets), "QR414"));

    let mut numbered_bullets = doc();
    numbered_bullets.blocks.push(paragraph(
        "numbered-bullets",
        "1. **目标**：明确方向\n2. **路径**：分解任务\n3. **保障**：跟踪结果",
    ));
    assert!(has_code(&lint_document(&numbered_bullets), "QR414"));
}

#[test]
fn qr414_detects_punctuated_and_four_item_enumerations() {
    for text in [
        "其一，方向要聚焦；其二，任务要分解；其三，结果要复盘。",
        "第一，明确目标；第二，压实责任；第三，跟踪评估。",
    ] {
        let mut document = doc();
        document
            .blocks
            .push(paragraph("punctuated-enumeration", text));
        assert!(
            has_code(&lint_document(&document), "QR414"),
            "punctuated enumeration must trigger: {text}"
        );
    }

    let mut four_items = doc();
    four_items.blocks.push(paragraph(
        "four-items",
        "一是2024年投入3亿元[^src_1]，二是产出显著增长，三是平台完成验收，四是队伍规模扩大。",
    ));
    assert!(
        has_code(&lint_document(&four_items), "QR414"),
        "hollow four-item enumerations must trigger even when they contain numbers and citations"
    );

    let mut ordinal_prose = doc();
    ordinal_prose.blocks.push(paragraph(
        "ordinal-prose",
        "全国第一，全省第二，冲击第三轮。",
    ));
    assert!(
        !has_code(&lint_document(&ordinal_prose), "QR414"),
        "ordinary ordinal prose without enumeration punctuation must remain unflagged"
    );
}

#[test]
fn qr414_allows_substantive_official_document_enumerations() {
    let mut detailed = doc();
    detailed.blocks.push(paragraph(
        "detailed-enumeration",
        "一是国家级科研项目由158项增至214项，重点围绕优势学科组织跨单位联合攻关，形成稳定的项目培育与质量把关机制；二是依托省重点实验室重组完善平台开放共享和成果转化流程，推动科研条件向本科生与研究生培养环节持续开放；三是实施高层次人才引育与青年教师支持计划，明确领军人才、骨干团队和青年教师的分层建设任务；四是建立年度监测、校内审核和第三方评价相结合的质量保障机制，将各项任务落实到责任单位与完成时限。",
    ));
    assert!(
        !has_code(&lint_document(&detailed), "QR414"),
        "substantive semicolon-delimited official-document items must remain allowed"
    );

    let mut padded = doc();
    let padded_text = format!(
        "一是新增国家级项目214项，{}；二是抓落实；三是促协同；四是强保障。",
        "持续完善机制".repeat(12)
    );
    padded
        .blocks
        .push(paragraph("padded-enumeration", &padded_text));
    assert!(
        has_code(&lint_document(&padded), "QR414"),
        "one padded item must not exempt otherwise hollow enumeration items"
    );
}

#[test]
fn qr415_requires_more_than_one_contrast_hit() {
    let mut single = doc();
    single
        .blocks
        .push(paragraph("single", "并非规模扩张，而是结构优化。"));
    assert!(
        !has_code(&lint_document(&single), "QR415"),
        "one contrast sentence must remain unflagged"
    );
}

#[test]
fn qr415_detects_repeated_contrast_patterns() {
    let mut dense = doc();
    dense.blocks.push(paragraph(
        "dense",
        "不是规模问题，而是结构问题；不是速度问题，而是质量问题。",
    ));
    let report = lint_document(&dense);
    let diagnostic = finding(&report, "QR415");
    assert!(diagnostic.message.contains("不是…而是…"));
    assert!(diagnostic.hint.contains("core fact"), "{}", diagnostic.hint);

    let mut speech = doc();
    speech.blocks.push(paragraph(
        "speech",
        "不是说规模不重要，而是说结构更关键；不是说速度不重要，而是说质量更关键。",
    ));
    let report = lint_document(&speech);
    assert!(finding(&report, "QR415").message.contains("不是说…而是说…"));

    let mut document_hits = doc();
    for index in 1..=3 {
        let id = format!("third-{index}");
        document_hits.blocks.push(paragraph(
            id.as_str(),
            "与其说速度决定成败，不如说结构决定长期表现。",
        ));
    }
    let report = lint_document(&document_hits);
    let hits: Vec<_> = report
        .diagnostics
        .iter()
        .filter(|diagnostic| diagnostic.code == "QR415")
        .collect();
    assert_eq!(hits.len(), 1, "{:?}", report.diagnostics);
    assert_eq!(hits[0].block_id.as_ref(), Some(&bid("third-3")));
}

#[test]
fn qr415_respects_gap_and_sentence_boundaries() {
    let mut too_far = doc();
    let far_text = format!("不是{}而是结构优化。", "数".repeat(46));
    too_far.blocks.push(paragraph("too-far", far_text.as_str()));
    assert!(
        !has_code(&lint_document(&too_far), "QR415"),
        "a gap over 45 characters must not match"
    );

    let mut cross_sentence = doc();
    cross_sentence
        .blocks
        .push(paragraph("cross-sentence", "不是规模问题。而是结构问题。"));
    assert!(
        !has_code(&lint_document(&cross_sentence), "QR415"),
        "contrast phrases split by a sentence boundary must not match"
    );
}

#[test]
fn qr415_detects_hedging_words() {
    for text in [
        "某种程度上，这项政策仍需观察。",
        "需要强调的是，数据口径尚未统一。",
    ] {
        let mut document = doc();
        document.blocks.push(paragraph("strong", text));
        assert!(
            has_code(&lint_document(&document), "QR415"),
            "strong hedge `{text}` must trigger"
        );
    }

    let mut single_light = doc();
    single_light
        .blocks
        .push(paragraph("single-light", "总体来看，进展顺利。"));
    assert!(!has_code(&lint_document(&single_light), "QR415"));

    let mut light_pair = doc();
    light_pair
        .blocks
        .push(paragraph("light-pair", "总体来看，诚然存在一些困难。"));
    let report = lint_document(&light_pair);
    let diagnostic = finding(&report, "QR415");
    assert!(diagnostic.message.contains("总体来看"));
    assert!(diagnostic.message.contains("诚然"));
}

#[test]
fn qr415_masks_quotes_and_skips_quote_blocks() {
    let mut document = doc();
    document.blocks.push(paragraph(
        "quoted",
        "“不是规模问题，而是结构问题；某种程度上需要观察。”",
    ));
    document.blocks.push(paragraph(
        "book",
        "《不是规模问题，而是结构问题》提供了另一种表述。",
    ));
    document.blocks.push(Block::quote(
        bid("external-quote"),
        "不是规模问题，而是结构问题；某种程度上需要观察。",
    ));
    assert!(
        !has_code(&lint_document(&document), "QR415"),
        "quoted and external contrast/hedging language must be masked"
    );
}

#[test]
fn qr415_can_be_block_ignored() {
    let mut document = doc();
    let mut block = paragraph(
        "ignored",
        "不是规模问题，而是结构问题；不是速度问题，而是质量问题。",
    );
    block.lint_ignore = vec!["QR415".into()];
    document.blocks.push(block);
    let report = lint_document(&document);
    assert!(!has_code(&report, "QR415"), "{:?}", report.diagnostics);
    assert!(report.summary.suppressed >= 1);
}

#[test]
fn qr412_qr413_qr414_can_be_block_ignored() {
    let mut cliche = doc();
    let mut cliche_block = paragraph("cliche", "综上所述，项目进展顺利。");
    cliche_block.lint_ignore = vec!["QR412".into()];
    cliche.blocks.push(cliche_block);
    let report = lint_document(&cliche);
    assert!(!has_code(&report, "QR412"), "{:?}", report.diagnostics);
    assert!(report.summary.suppressed >= 1);

    let long_text = "数".repeat(130);
    let mut evidence = doc();
    let mut evidence_block = paragraph("evidence", long_text.as_str());
    evidence_block.lint_ignore = vec!["QR413".into()];
    evidence.blocks.push(evidence_block);
    let report = lint_document(&evidence);
    assert!(!has_code(&report, "QR413"), "{:?}", report.diagnostics);
    assert!(report.summary.suppressed >= 1);

    let mut template = doc();
    let mut template_block = paragraph("template", "首先明确目标，其次分解任务，最后跟踪结果。");
    template_block.lint_ignore = vec!["QR414".into()];
    template.blocks.push(template_block);
    let report = lint_document(&template);
    assert!(!has_code(&report, "QR414"), "{:?}", report.diagnostics);
    assert!(report.summary.suppressed >= 1);
}

#[test]
fn qr417_flags_only_single_paragraph_walls_of_text() {
    let wall = "数".repeat(450);
    let mut single = doc();
    single.blocks.push(paragraph("wall", wall.as_str()));
    let sentence = format!("{}。", "句".repeat(49));
    let mut multi_sentence = doc();
    multi_sentence
        .blocks
        .push(paragraph("multi-sentence", sentence.repeat(6).as_str()));
    let report = lint_document(&multi_sentence);
    assert!(
        has_code(&report, "QR417"),
        "300 visible characters across six sentences form a multi-idea wall: {:?}",
        report.diagnostics
    );

    let mut shorter = doc();
    shorter
        .blocks
        .push(paragraph("shorter", sentence.repeat(5).as_str()));
    assert!(!has_code(&lint_document(&shorter), "QR417"));

    let mut long_single_sentence = doc();
    long_single_sentence
        .blocks
        .push(paragraph("long-single", "数".repeat(360).as_str()));
    assert!(has_code(&lint_document(&long_single_sentence), "QR417"));

    let report = lint_document(&single);
    let diagnostic = finding(&report, "QR417");
    assert_eq!(diagnostic.severity, Severity::Warning);
    assert!(diagnostic.suppressible);
    assert!(
        diagnostic.message.contains("450-char"),
        "{}",
        diagnostic.message
    );

    let long_url = format!("https://example.com/{}", "very-long-path".repeat(20));
    let mut linked = String::new();
    for _ in 0..20 {
        linked.push_str("[链接](");
        linked.push_str(&long_url);
        linked.push_str(")[^src_1]");
    }
    let mut links = doc();
    links.blocks.push(paragraph("links", linked.as_str()));
    assert!(
        !has_code(&lint_document(&links), "QR417"),
        "long URLs and citation keys must not inflate the prose length"
    );

    let mut listed = "数".repeat(460);
    listed.push_str("\n- 第一条\n- 第二条");
    let mut list_doc = doc();
    list_doc.blocks.push(paragraph("listed", listed.as_str()));
    assert!(!has_code(&lint_document(&list_doc), "QR417"));

    let mut run = doc();
    let paragraph_text = "数".repeat(300);
    for index in 1..=5 {
        let id = format!("run-{index}");
        run.blocks
            .push(paragraph(id.as_str(), paragraph_text.as_str()));
    }
    let report = lint_document(&run);
    assert!(
        !has_code(&report, "QR417"),
        "multi-paragraph density belongs to QR419: {:?}",
        report.diagnostics
    );
    let dense_hits: Vec<_> = report
        .diagnostics
        .iter()
        .filter(|diagnostic| diagnostic.code == "QR419")
        .collect();
    assert_eq!(dense_hits.len(), 1, "{:?}", report.diagnostics);
    assert_eq!(
        dense_hits[0].block_id.as_ref(),
        Some(&bid("run-5")),
        "QR419 must retain the cross-paragraph diagnostic"
    );
}

#[test]
fn qr419_flags_dense_prose_runs_with_section_exemptions() {
    let text = "数".repeat(400);
    let mut normal = doc();
    normal.blocks.push(heading("dense-h2", 2, "规模结构"));
    for index in 1..=4 {
        let id = format!("dense-{index}");
        normal.blocks.push(paragraph(id.as_str(), text.as_str()));
    }
    let report = lint_document(&normal);
    let diagnostic = finding(&report, "QR419");
    assert_eq!(diagnostic.severity, Severity::Warning);
    assert!(diagnostic.suppressible);
    assert!(
        diagnostic.message.contains("4-block (1600-char)"),
        "{}",
        diagnostic.message
    );
    assert!(diagnostic.hint.contains("Table or ECharts"));
    assert!(diagnostic.hint.contains("Never insert a duplicate"));

    let concise_text = "数".repeat(200);
    let mut concise = doc();
    concise.blocks.push(heading("concise-h2", 2, "规模结构"));
    for index in 1..=5 {
        concise.blocks.push(paragraph(
            &format!("concise-{index}"),
            concise_text.as_str(),
        ));
    }
    assert!(
        !has_code(&lint_document(&concise), "QR419"),
        "focused short paragraphs must not be treated as a wall merely for totaling 1000 chars"
    );

    for title in [
        "导论",
        "结语",
        "四、结语",
        "总结与展望",
        "对策建议",
        "对策建议：依托省重点实验室集中引育领军人才",
        "存在问题与下一步工作",
        "保障措施",
        "落实要求",
        "建设路径：由规模扩张转向层级跃迁效率",
        "布局优化路径",
        "工作举措",
    ] {
        let mut exempt = doc();
        exempt.blocks.push(heading("exempt-h2", 2, title));
        for index in 1..=4 {
            let id = format!("exempt-{index}");
            exempt.blocks.push(paragraph(id.as_str(), text.as_str()));
        }
        assert!(
            !has_code(&lint_document(&exempt), "QR419"),
            "section `{title}` must be exempt"
        );
    }

    let long_text = "数".repeat(400);
    let mut reset = doc();
    reset.blocks.push(heading("reset-h2", 2, "规模结构"));
    for index in 1..=3 {
        let id = format!("before-table-{index}");
        reset
            .blocks
            .push(paragraph(id.as_str(), long_text.as_str()));
    }
    reset.blocks.push(table(
        "reset-table",
        None,
        vec![ColumnNode::leaf("a", "A")],
        vec![row(&[("a", CellValue::text("x"))])],
    ));
    for index in 1..=4 {
        let id = format!("after-table-{index}");
        reset.blocks.push(paragraph(id.as_str(), text.as_str()));
    }
    let report = lint_document(&reset);
    let hits: Vec<_> = report
        .diagnostics
        .iter()
        .filter(|diagnostic| diagnostic.code == "QR419")
        .collect();
    assert_eq!(hits.len(), 1, "{:?}", report.diagnostics);
    assert_eq!(hits[0].block_id.as_ref(), Some(&bid("after-table-4")));

    let mut visually_served = doc();
    visually_served
        .blocks
        .push(heading("visual-section", 1, "规模结构"));
    for (index, id) in ["visual-a", "visual-b"].iter().enumerate() {
        visually_served.blocks.push(table(
            id,
            None,
            vec![ColumnNode::leaf("value", "值")],
            vec![row(&[("value", CellValue::Integer(index as i64 + 1))])],
        ));
        visually_served.blocks.push(paragraph(
            &format!("visual-bridge-{index}"),
            "图表之间的解释。",
        ));
    }
    for index in 1..=4 {
        visually_served
            .blocks
            .push(paragraph(&format!("visual-prose-{index}"), text.as_str()));
    }
    assert!(
        !has_code(&lint_document(&visually_served), "QR419"),
        "a section already served by two visuals must not be forced to add another"
    );

    let mut h1_path = doc();
    h1_path
        .blocks
        .push(heading("h1-path", 1, "建设路径：提高层级跃迁效率"));
    for index in 1..=4 {
        h1_path
            .blocks
            .push(paragraph(&format!("h1-path-{index}"), text.as_str()));
    }
    assert!(!has_code(&lint_document(&h1_path), "QR419"));

    let medium_text = "数".repeat(400);
    let mut h3 = doc();
    h3.blocks.push(heading("h3-section", 2, "规模结构"));
    h3.blocks
        .push(paragraph("h3-before-1", medium_text.as_str()));
    h3.blocks
        .push(paragraph("h3-before-2", medium_text.as_str()));
    h3.blocks.push(heading("h3-sub", 3, "细分结构"));
    h3.blocks.push(paragraph("h3-after-1", text.as_str()));
    h3.blocks.push(paragraph("h3-after-2", text.as_str()));
    let report = lint_document(&h3);
    assert_eq!(
        finding(&report, "QR419").block_id.as_ref(),
        Some(&bid("h3-after-2"))
    );

    let mut ignored = doc();
    ignored.blocks.push(heading("ignored-h2", 2, "规模结构"));
    for index in 1..=4 {
        let id = format!("ignored-{index}");
        let mut block = paragraph(id.as_str(), text.as_str());
        if index == 4 {
            block.lint_ignore = vec!["QR419".into()];
        }
        ignored.blocks.push(block);
    }
    let report = lint_document(&ignored);
    assert!(!has_code(&report, "QR419"), "{:?}", report.diagnostics);
    assert!(report.summary.suppressed >= 1);
}

#[test]
fn qr426_flags_editorial_meta_labels_without_banning_method_notes() {
    let mut flagged = doc();
    for (index, text) in [
        "专业学位博士点为0个（表 1，核实值）。",
        "按支撑强度排序投入（本报告建议）。",
        "据此本报告判断，单点突破更可行。",
        "这一解释仍需验证（属推断）。",
        "本报告不作逐条引证。",
        "本报告不将其计入既有事实。",
    ]
    .iter()
    .enumerate()
    {
        flagged
            .blocks
            .push(paragraph(&format!("meta-{index}"), text));
    }
    flagged.blocks.push(table(
        "meta-title",
        Some("三项中间标志（本报告建议）"),
        vec![ColumnNode::leaf("a", "观察项")],
        vec![row(&[("a", CellValue::text("团队"))])],
    ));
    flagged.blocks.push(table(
        "meta-column",
        Some("行动安排"),
        vec![ColumnNode::leaf("a", "可核验信号（本报告建议）")],
        vec![row(&[("a", CellValue::text("年度台账"))])],
    ));

    let report = lint_document(&flagged);
    let hits: Vec<_> = report
        .diagnostics
        .iter()
        .filter(|diagnostic| diagnostic.code == "QR426")
        .collect();
    assert_eq!(hits.len(), 8, "{:?}", report.diagnostics);
    assert!(
        hits.iter()
            .all(|diagnostic| diagnostic.severity == Severity::Warning && diagnostic.suppressible)
    );

    let mut clean = doc();
    clean.blocks.push(paragraph(
        "scope",
        "本报告以2021—2025年为统计窗口，建议集中资源建设涉海学科。",
    ));
    clean.blocks.push(paragraph(
        "quoted",
        "“本报告建议”属于需要清理的内部编辑标签。",
    ));
    clean.blocks.push(with_table_note(
        table(
            "method-note",
            Some("阶段目标（本报告测算）"),
            vec![ColumnNode::leaf("a", "指标")],
            vec![row(&[("a", CellValue::text("人才"))])],
        ),
        "阶段目标为本报告建议值，非官方下达指标。",
    ));
    let report = lint_document(&clean);
    assert!(!has_code(&report, "QR426"), "{:?}", report.diagnostics);

    let mut ignored = paragraph("ignored-meta", "按支撑强度排序（本报告建议）。");
    ignored.lint_ignore = vec!["QR426".into()];
    let mut ignored_document = doc();
    ignored_document.blocks.push(ignored);
    let report = lint_document(&ignored_document);
    assert!(!has_code(&report, "QR426"));
    assert_eq!(report.summary.suppressed, 1);
}

#[test]
fn qr421_detects_blind_ordinals_and_academic_jargon() {
    for (id, text) in [
        ("blind-pair", "第一项与第三项占优。"),
        ("blind-metric", "第二项指标表现突出。"),
        ("blind-shortcoming", "第四项存在短板。"),
        ("spearman", "采用Spearman相关系数分析。"),
        ("sample", "样本高校覆盖不同类型院校。"),
        ("p-value", "回归结果满足p < 0.05。"),
        ("hhi", "HHI反映学科集中度。"),
    ] {
        let mut document = doc();
        document.blocks.push(paragraph(id, text));
        assert!(
            has_code(&lint_document(&document), "QR421"),
            "blind ordinal or academic jargon must be detected: {text}"
        );
    }

    let mut spearman = doc();
    spearman.blocks.push(paragraph(
        "spearman-single-owner",
        "采用Spearman相关系数分析。",
    ));
    assert!(
        !has_code(&lint_document(&spearman), "QR412"),
        "QR421 owns academic jargon without duplicating QR412"
    );

    for (id, text) in [
        ("named-quoted", "第一项“国家级项目数”占优。"),
        ("named-plain", "第一项立项数占优。"),
        ("defined", "第一项为立项数，第二项为经费总量。"),
        ("quoted-title", "《第一项与第三项占优》只是待讨论标题。"),
        ("ordinary-rank", "2025年立项数位居全省第一。"),
        ("quoted-method", "《回归分析》是本次引用的资料名称。"),
    ] {
        let mut document = doc();
        document.blocks.push(paragraph(id, text));
        assert!(
            !has_code(&lint_document(&document), "QR421"),
            "named metrics and quoted material must remain clean: {text}"
        );
    }
}

#[test]
fn qr422_requires_visual_units_or_scope_note() {
    let mut table_missing = doc();
    let mut missing_table_block = table(
        "missing-table-note",
        None,
        vec![
            ColumnNode::leaf("name", "名称"),
            ColumnNode::leaf("count", "项目数").with_type(ColumnDataType::Number),
        ],
        vec![row(&[
            ("name", CellValue::text("甲")),
            ("count", CellValue::Integer(214)),
        ])],
    );
    missing_table_block.source_note = Some("教育部".into());
    table_missing.blocks.push(missing_table_block);
    let report = lint_document(&table_missing);
    let diagnostic = finding(&report, "QR422");
    assert_eq!(diagnostic.severity, Severity::Warning);
    assert!(diagnostic.suppressible);
    assert_eq!(diagnostic.path.as_deref(), Some("blocks[0].note"));

    let mut table_with_units = doc();
    let mut units_table_block = table(
        "table-with-units",
        None,
        vec![
            ColumnNode::leaf("name", "名称"),
            ColumnNode::leaf("count", "项目数（项）").with_type(ColumnDataType::Number),
        ],
        vec![row(&[
            ("name", CellValue::text("甲")),
            ("count", CellValue::Integer(214)),
        ])],
    );
    units_table_block.source_note = Some("教育部".into());
    table_with_units.blocks.push(units_table_block);
    assert!(!has_code(&lint_document(&table_with_units), "QR422"));

    let mut table_with_note = doc();
    let mut noted_table_block = with_table_note(
        table(
            "table-with-note",
            None,
            vec![
                ColumnNode::leaf("name", "名称"),
                ColumnNode::leaf("count", "项目数").with_type(ColumnDataType::Number),
            ],
            vec![row(&[
                ("name", CellValue::text("甲")),
                ("count", CellValue::Integer(214)),
            ])],
        ),
        "单位：项；统计口径：2025年国家级项目。",
    );
    noted_table_block.source_note = Some("教育部".into());
    table_with_note.blocks.push(noted_table_block);
    assert!(!has_code(&lint_document(&table_with_note), "QR422"));

    let mut chart_missing = doc();
    let mut missing_chart_block = chart_named(
        "chart-without-units",
        serde_json::json!({
            "xAxis": {"type": "category", "data": ["A", "B"]},
            "yAxis": {"type": "value"},
            "series": [{"type": "bar", "data": [1, 2]}]
        }),
    );
    missing_chart_block.source_note = Some("教育部".into());
    chart_missing.blocks.push(missing_chart_block);
    assert!(has_code(&lint_document(&chart_missing), "QR422"));

    let mut chart_with_units = doc();
    let mut units_chart_block = chart_named(
        "chart-with-units",
        serde_json::json!({
            "xAxis": {"type": "category", "data": ["A", "B"]},
            "yAxis": {"type": "value", "name": "人才数（名）"},
            "series": [{"type": "bar", "data": [1, 2]}]
        }),
    );
    units_chart_block.source_note = Some("教育部".into());
    chart_with_units.blocks.push(units_chart_block);
    assert!(!has_code(&lint_document(&chart_with_units), "QR422"));

    let mut pie_with_units = doc();
    let mut pie_block = chart_named(
        "pie-with-units",
        serde_json::json!({
            "series": [{"name": "项目数（项）", "type": "pie", "data": [{"value": 1}, {"value": 2}]}]
        }),
    );
    pie_block.source_note = Some("教育部".into());
    pie_with_units.blocks.push(pie_block);
    assert!(!has_code(&lint_document(&pie_with_units), "QR422"));
}

#[test]
fn qr422_requires_visual_sources_and_bounds_note_length() {
    let make_table = |id: &str| {
        table(
            id,
            None,
            vec![ColumnNode::leaf("count", "项目数（项）").with_type(ColumnDataType::Number)],
            vec![row(&[("count", CellValue::Integer(214))])],
        )
    };
    let make_chart = |id: &str| {
        chart_named(
            id,
            serde_json::json!({
                "xAxis": {"type": "category", "data": ["A", "B"]},
                "yAxis": {"type": "value", "name": "项目数（项）"},
                "series": [{"type": "bar", "data": [1, 2]}]
            }),
        )
    };

    let mut missing_table_source = doc();
    let mut table_block = make_table("missing-table-source");
    table_block.source_note = Some("  \n".into());
    missing_table_source.blocks.push(table_block);
    let report = lint_document(&missing_table_source);
    let diagnostic = finding(&report, "QR422");
    assert_eq!(diagnostic.severity, Severity::Warning);
    assert_eq!(diagnostic.path.as_deref(), Some("blocks[0].source_note"));
    assert!(diagnostic.message.contains("source_ids"));
    assert!(diagnostic.hint.contains("资料来源"));

    let mut missing_chart_source = doc();
    missing_chart_source
        .blocks
        .push(make_chart("missing-chart-source"));
    let report = lint_document(&missing_chart_source);
    let diagnostic = finding(&report, "QR422");
    assert_eq!(diagnostic.path.as_deref(), Some("blocks[0].source_note"));

    let mut long_table_note = doc();
    let mut table_block = make_table("long-table-note");
    table_block.source_note = Some("教育部".into());
    if let BlockKind::Table(table) = &mut table_block.body {
        table.note = Some("长".repeat(181));
    }
    long_table_note.blocks.push(table_block);
    let report = lint_document(&long_table_note);
    let diagnostic = finding(&report, "QR422");
    assert_eq!(diagnostic.path.as_deref(), Some("blocks[0].note"));
    assert!(diagnostic.message.contains("当前 181 字"));
    assert!(diagnostic.hint.contains("120 字（2 行）以内"));
    assert!(diagnostic.hint.contains("重复缺失说明"));

    let mut long_chart_note = doc();
    let mut chart_block = make_chart("long-chart-note");
    chart_block.source_note = Some("教育部".into());
    if let BlockKind::Echarts(chart) = &mut chart_block.body {
        chart.note = Some("长".repeat(181));
    }
    long_chart_note.blocks.push(chart_block);
    let report = lint_document(&long_chart_note);
    let diagnostic = finding(&report, "QR422");
    assert!(diagnostic.message.contains("当前 181 字"));

    let mut boundary_note = doc();
    let mut table_block = make_table("boundary-table-note");
    table_block.source_note = Some("教育部".into());
    if let BlockKind::Table(table) = &mut table_block.body {
        table.note = Some("长".repeat(180));
    }
    boundary_note.blocks.push(table_block);
    assert!(!has_code(&lint_document(&boundary_note), "QR422"));
}

#[test]
fn qr423_enforces_policy_terms_year_ranges_and_document_numbers() {
    let mut invalid = doc();
    invalid.blocks.push(paragraph(
        "invalid-policy-format",
        "双一流建设应衔接十四五规划和十五五布局，参照教发(2025)12号，分析2021-2025年数据。",
    ));
    let report = lint_document(&invalid);
    let diagnostic = finding(&report, "QR423");
    assert!(diagnostic.message.contains("“双一流”"));
    assert!(diagnostic.message.contains("“十四五”"));
    assert!(diagnostic.message.contains("“十五五”"));
    assert!(diagnostic.message.contains("一字线"));
    assert!(diagnostic.message.contains("六角括号"));

    for (id, text) in [
        ("tilde-range", "分析2021~2025年数据。"),
        ("fullwidth-tilde-range", "分析2021～2025年数据。"),
        ("fullwidth-doc-number", "参照教发字（2025）12号。"),
    ] {
        let mut document = doc();
        document.blocks.push(paragraph(id, text));
        assert!(has_code(&lint_document(&document), "QR423"));
    }

    let mut valid = doc();
    valid.blocks.push(paragraph(
        "valid-policy-format",
        "“双一流”建设衔接“十四五”规划和“十五五”布局，参照教发〔2025〕12号，分析2021—2025年数据。",
    ));
    assert!(!has_code(&lint_document(&valid), "QR423"));

    let mut quoted = doc();
    quoted.blocks.push(Block::quote(
        bid("external-policy-quote"),
        "双一流建设参见十四五规划。",
    ));
    quoted
        .blocks
        .push(paragraph("code", "`双一流` 是代码示例。"));
    assert!(!has_code(&lint_document(&quoted), "QR423"));
}

#[test]
fn qr423_scans_headings_table_and_chart_notes() {
    let mut heading_doc = doc();
    heading_doc
        .blocks
        .push(heading("policy-heading", 1, "双一流建设"));
    assert!(has_code(&lint_document(&heading_doc), "QR423"));

    let mut table_doc = doc();
    let mut table_block = with_table_note(
        table(
            "policy-table",
            Some("十四五指标"),
            vec![ColumnNode::leaf("name", "名称")],
            vec![row(&[("name", CellValue::text("A"))])],
        ),
        "十五五布局",
    );
    table_block.source_note = Some("双一流政策依据".into());
    table_doc.blocks.push(table_block);
    let table_paths: Vec<String> = lint_document(&table_doc)
        .diagnostics
        .iter()
        .filter(|diagnostic| diagnostic.code == "QR423")
        .filter_map(|diagnostic| diagnostic.path.clone())
        .collect();
    for path in ["blocks[0].title", "blocks[0].note", "blocks[0].source_note"] {
        assert!(table_paths.contains(&path.to_string()), "{table_paths:?}");
    }

    let mut chart_doc = doc();
    let mut chart_block = chart_named(
        "policy-chart",
        serde_json::json!({"series": [{"type": "bar", "data": [1]}]}),
    );
    chart_block.source_note = Some("十五五政策依据".into());
    if let BlockKind::Echarts(chart) = &mut chart_block.body {
        chart.title = Some("双一流对比".into());
        chart.note = Some("十四五期间".into());
    }
    chart_doc.blocks.push(chart_block);
    let chart_paths: Vec<String> = lint_document(&chart_doc)
        .diagnostics
        .iter()
        .filter(|diagnostic| diagnostic.code == "QR423")
        .filter_map(|diagnostic| diagnostic.path.clone())
        .collect();
    for path in ["blocks[0].title", "blocks[0].note", "blocks[0].source_note"] {
        assert!(chart_paths.contains(&path.to_string()), "{chart_paths:?}");
    }
}

#[test]
fn qr424_scans_all_visible_publication_fields() {
    let mut document = doc();
    document.blocks.push(heading(
        "internal-heading",
        1,
        "学科云index系统对比（record_id=42）",
    ));
    document.blocks.push(paragraph(
        "internal-paragraph",
        "统计口径 target_id=0、target_level=0、includeSub=true。",
    ));
    document.blocks.push(Block::quote(
        bid("internal-quote"),
        "内部编码 local:4.1.1 不应进入交付文本。",
    ));

    let mut table_block = with_summary_row(
        with_table_note(
            table(
                "internal-table",
                Some("province=广东，detail=matrix"),
                vec![ColumnNode::leaf("name", "名称")],
                vec![
                    row(&[("name", CellValue::text("学科云无数据"))]),
                    row(&[("name", CellValue::text("2021=4"))]),
                ],
            ),
            "LATEST期末快照，字段 FacultyInfo。",
        ),
        row(&[("name", CellValue::text("2026=43"))]),
    );
    table_block.source_note = Some("indicator_id、block_id、source_id、record_id".into());
    document.blocks.push(table_block);

    let mut chart_block = chart_named(
        "internal-chart",
        serde_json::json!({"series": [{"type": "bar", "data": [1]}]}),
    );
    chart_block.source_note = Some("target_level=0".into());
    if let BlockKind::Echarts(chart) = &mut chart_block.body {
        chart.title = Some("左panel与右panel".into());
        chart.note = Some("NOT_COLLECTED".into());
        chart.fallback_summary = "学科云index系统，includeSub=true".into();
    }
    document.blocks.push(chart_block);

    let mut html = custom_html("<p>safe</p>", "双优库无数据；NOT_COLLECTED");
    html.source_note = Some("record_id=42".into());
    if let BlockKind::CustomHtml(custom) = &mut html.body {
        custom.title = Some("右panel".into());
        custom.note = Some("local:3.1.1".into());
    }
    document.blocks.push(html);

    let report = lint_document(&document);
    let hits: Vec<_> = report
        .diagnostics
        .iter()
        .filter(|diagnostic| diagnostic.code == "QR424")
        .collect();
    assert!(hits.len() >= 12, "{:?}", report.diagnostics);
    assert!(
        hits.iter().all(|diagnostic| {
            diagnostic.severity == Severity::Error && !diagnostic.suppressible
        })
    );
    for path in [
        "blocks[0].text",
        "blocks[1].text",
        "blocks[2].text",
        "blocks[3].title",
        "blocks[3].note",
        "blocks[3].source_note",
        "blocks[3].rows[0].name",
        "blocks[3].summary_rows[0].name",
        "blocks[4].title",
        "blocks[4].note",
        "blocks[4].source_note",
        "blocks[4].fallback_summary",
        "blocks[5].title",
        "blocks[5].note",
        "blocks[5].source_note",
        "blocks[5].fallback_text",
    ] {
        assert!(
            hits.iter()
                .any(|diagnostic| diagnostic.path.as_deref() == Some(path)),
            "missing {path}: {:?}",
            report.diagnostics
        );
    }
}

#[test]
fn qr424_is_not_suppressible_and_accepts_business_chinese() {
    let mut document = doc();
    document.lint.ignore = vec!["QR424".into()];
    let mut block = paragraph("unsafe", "record_id=42");
    block.lint_ignore = vec!["internal-identifier-leak".into()];
    document.blocks.push(block);
    assert!(has_code(&lint_document(&document), "QR424"));

    for text in [
        "统计口径含参与单位与附属机构",
        "高层次人才称号记录",
        "左图与右图",
        "2021—2026年暂未收录指标以—表示",
    ] {
        let mut document = doc();
        document.blocks.push(paragraph("business-language", text));
        assert!(!has_code(&lint_document(&document), "QR424"), "{text}");
    }
}

#[test]
fn qr419_inherits_action_path_h1_exemption_across_child_h2_sections() {
    let text = "数".repeat(420);
    let mut document = doc();
    document.blocks.push(heading(
        "ch6",
        1,
        "行动路径：在海工装备与远洋渔业两条线上集中破局",
    ));
    document.blocks.push(heading(
        "c6-2",
        2,
        "主攻方向：围绕四大省级平台配置学科交叉增量",
    ));
    for index in 1..=5 {
        document
            .blocks
            .push(paragraph(&format!("c6-2-p{index}"), text.as_str()));
    }
    assert!(
        !has_code(&lint_document(&document), "QR419"),
        "child H2 inside an action-path H1 chapter must inherit the QR419 exemption"
    );
}

#[test]
fn qr426_catches_section_boundary_meta_narration() {
    for text in [
        "存量目标与缺口测算在行动路径章统一处理，本节只确认增长节奏与相对位置这两件事。",
        "需要在申报口径上收敛的一处细节是省部级平台数量。",
        "这一位置决定了它在申报中的用法，也解释了第三节的口径差异。",
        "这一对应是基于学科名称的推断，正式申报前应由教务处核对。",
    ] {
        let mut document = doc();
        document.blocks.push(paragraph("meta", text));
        assert!(
            has_code(&lint_document(&document), "QR426"),
            "meta narration `{text}` must trigger QR426"
        );
    }
}

#[test]
fn qr417_does_not_split_sentences_on_semicolons() {
    let mut document = doc();
    // 4 full sentences separated by `。`, with 2 semicolons `；` inside (304 chars total)
    let para = format!(
        "{}；{}。{}。{}；{}。{}。",
        "甲".repeat(50),
        "乙".repeat(50),
        "丙".repeat(50),
        "丁".repeat(50),
        "戊".repeat(50),
        "己".repeat(48),
    );
    document.blocks.push(paragraph("c3p07", &para));
    let report = lint_document(&document);
    assert!(
        !has_code(&report, "QR417"),
        "a 4-sentence 304-char paragraph with semicolons must not be miscounted as 6 sentences: {:?}",
        report.diagnostics
    );
}
