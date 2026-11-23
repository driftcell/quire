#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::indexing_slicing,
    clippy::panic
)]

pub mod common;
use common::*;

#[test]
fn structure_and_text_rules_are_detected() {
    let mut placeholder = doc();
    placeholder.blocks.push(paragraph("p", "Revenue is TBD."));
    assert!(has_code(&lint_document(&placeholder), "QR401"));

    let mut jump = doc();
    jump.blocks.push(heading("h1", 1, "One"));
    jump.blocks.push(heading("h3", 3, "Three"));
    assert!(has_code(&lint_document(&jump), "QR402"));

    let mut empty_section = doc();
    empty_section.blocks.push(heading("a", 1, "A"));
    empty_section.blocks.push(heading("b", 1, "B"));
    assert!(has_code(&lint_document(&empty_section), "QR403"));

    let mut numbering = doc();
    numbering.blocks.push(heading("h1", 1, "1.1 Overview"));
    assert!(has_code(&lint_document(&numbering), "QR404"));

    let mut sequence = doc();
    sequence.blocks.push(table(
        "t1",
        Some("表 1 Revenue"),
        vec![ColumnNode::leaf("a", "A")],
        vec![],
    ));
    sequence.blocks.push(table(
        "t3",
        Some("表 3 Cost"),
        vec![ColumnNode::leaf("a", "A")],
        vec![],
    ));
    let sequence_report = lint_document(&sequence);
    assert!(!has_code(&sequence_report, "QR405"));
    assert_eq!(
        sequence_report
            .diagnostics
            .iter()
            .filter(|diagnostic| diagnostic.code == "QR411")
            .count(),
        2,
        "each manual caption must produce only QR411: {:?}",
        sequence_report.diagnostics
    );

    let mut dangling_ref = doc();
    dangling_ref
        .blocks
        .push(paragraph("ref", "See 表 3 for details."));
    assert!(has_code(&lint_document(&dangling_ref), "QR406"));

    let mut unclosed = doc();
    unclosed.blocks.push(paragraph("bold", "Not **closed"));
    assert!(has_code(&lint_document(&unclosed), "QR407"));

    let mut empty_text = doc();
    empty_text.blocks.push(paragraph("empty", "   "));
    assert!(has_code(&lint_document(&empty_text), "QR408"));

    let mut empty_heading = doc();
    empty_heading.blocks.push(heading("empty-heading", 1, ""));
    assert!(has_code(&lint_document(&empty_heading), "QR409"));

    assert!(has_code(&lint_document(&doc()), "QR410"));

    let mut manual = doc();
    manual.blocks.push(table(
        "manual",
        Some("表 1 Revenue"),
        vec![ColumnNode::leaf("a", "A")],
        vec![],
    ));
    assert!(has_code(&lint_document(&manual), "QR411"));
}

#[test]
fn qr404_supports_relative_hierarchies_and_checks_sibling_sequences() {
    let mut chinese = doc();
    for (id, level, text) in [
        ("report", 1, "学科建设报告"),
        ("chapter-1", 2, "一、总体情况"),
        ("section-1", 3, "（一）师资队伍"),
        ("item-1", 4, "1. 领军人才"),
        ("item-2", 4, "2. 青年教师"),
        ("section-2", 3, "（二）学科平台"),
        ("chapter-2", 2, "二、建设成效"),
        ("chapter-2-section", 3, "（一）科研项目"),
    ] {
        chinese.blocks.push(heading(id, level, text));
    }
    assert!(
        !has_code(&lint_document(&chinese), "QR404"),
        "an unnumbered H1 must allow H2-rooted official-document numbering"
    );

    let mut arabic = doc();
    for (id, level, text) in [
        ("report", 1, "1. 总体情况"),
        ("section-1", 2, "1.1 师资队伍"),
        ("section-2", 2, "1.2 学科平台"),
    ] {
        arabic.blocks.push(heading(id, level, text));
    }
    assert!(!has_code(&lint_document(&arabic), "QR404"));

    let mut duplicate = doc();
    duplicate.blocks.push(heading("first", 2, "一、总体情况"));
    duplicate
        .blocks
        .push(heading("duplicate", 2, "一、建设成效"));
    let duplicate_report = lint_document(&duplicate);
    assert!(
        finding(&duplicate_report, "QR404")
            .message
            .contains("does not follow")
    );

    let mut gap = doc();
    gap.blocks.push(heading("first", 2, "一、总体情况"));
    gap.blocks.push(heading("gap", 2, "三、建设成效"));
    assert!(has_code(&lint_document(&gap), "QR404"));

    let mut extended = doc();
    for (index, numeral) in [
        "一", "二", "三", "四", "五", "六", "七", "八", "九", "十", "十一",
    ]
    .iter()
    .enumerate()
    {
        let id = format!("chapter-{index}");
        extended
            .blocks
            .push(heading(&id, 2, &format!("{numeral}、建设任务")));
    }
    assert!(
        !has_code(&lint_document(&extended), "QR404"),
        "multi-character Chinese numerals must remain in sequence"
    );

    let mut decimal = doc();
    decimal
        .blocks
        .push(heading("decimal-percent", 2, "3.5% 增速背后的结构性因素"));
    assert!(!has_code(&lint_document(&decimal), "QR404"));
}

#[test]
fn qr401_detects_higher_education_template_placeholders_conservatively() {
    for (id, text) in [
        ("xx-university", "XX大学应补充正式名称。"),
        ("cjk-mask", "××学院应补充正式名称。"),
        ("pending-brackets", "【待填写】应替换为已核实内容。"),
        ("pending-parentheses", "（待补全）应替换为已核实内容。"),
        ("n-quantifier", "计划引育 N 名高层次人才。"),
        ("missing-percent", "占比为 ?%。"),
        ("question-placeholder", "具体数值为??。"),
    ] {
        let mut document = doc();
        document.blocks.push(paragraph(id, text));
        assert!(
            has_code(&lint_document(&document), "QR401"),
            "unfinished template placeholder must be detected: {text}"
        );
    }

    for (id, text) in [
        ("anonymous", "某高校在匿名对标中位居前列。"),
        (
            "angle-example",
            "<学校名称>仅作为格式示例，不作为模板槽位拦截。",
        ),
        ("n-value", "N 值表示总体规模，不是待填数量。"),
        ("pending-action", "方案待批复后实施。"),
    ] {
        let mut document = doc();
        document.blocks.push(paragraph(id, text));
        assert!(
            !has_code(&lint_document(&document), "QR401"),
            "intentional prose must not become a placeholder error: {text}"
        );
    }
}

#[test]
fn qr406_rejects_zero_compound_and_appendix_visual_references() {
    let mut document = doc();
    for id in ["table-1", "table-2", "table-3"] {
        document.blocks.push(table(
            id,
            None,
            vec![ColumnNode::leaf("value", "数值")],
            vec![],
        ));
    }
    document.blocks.push(paragraph(
        "references",
        "表 3 可直接定位；表 3.2、图 1.3、表 0、附表 1、Table 2-1、Figure 1.2 均需改写。",
    ));
    let report = lint_document(&document);
    let hits: Vec<_> = report
        .diagnostics
        .iter()
        .filter(|diagnostic| diagnostic.code == "QR406")
        .collect();
    assert_eq!(hits.len(), 6, "{:?}", report.diagnostics);
    for raw in [
        "表 3.2",
        "图 1.3",
        "表 0",
        "附表 1",
        "table 2-1",
        "figure 1.2",
    ] {
        assert!(
            hits.iter()
                .any(|diagnostic| diagnostic.message.contains(raw)),
            "missing complete reference `{raw}`: {:?}",
            report.diagnostics
        );
    }
}

#[test]
fn qr416_flags_third_consecutive_visual_only() {
    let mut two = doc();
    two.blocks.push(table(
        "table-1",
        None,
        vec![ColumnNode::leaf("a", "A")],
        vec![row(&[("a", CellValue::text("x"))])],
    ));
    two.blocks.push(chart_named(
        "chart-1",
        serde_json::json!({"series": [{"type": "bar", "data": [1]}]}),
    ));
    assert!(!has_code(&lint_document(&two), "QR416"));

    let mut three = doc();
    three.blocks.push(heading("h-a", 2, "Dataset A"));
    three.blocks.push(table(
        "table-2",
        None,
        vec![ColumnNode::leaf("a", "A")],
        vec![row(&[("a", CellValue::text("x"))])],
    ));
    three.blocks.push(heading("h-b", 3, "Dataset B"));
    three.blocks.push(chart_named(
        "chart-2",
        serde_json::json!({"series": [{"type": "bar", "data": [1]}]}),
    ));
    three.blocks.push(heading("h-c", 3, "Dataset C"));
    let mut third = custom_html("<div>KPI</div>", "KPI");
    third.id = bid("html-3");
    three.blocks.push(third);
    let report = lint_document(&three);
    let hits: Vec<_> = report
        .diagnostics
        .iter()
        .filter(|diagnostic| diagnostic.code == "QR416")
        .collect();
    assert_eq!(hits.len(), 1, "{:?}", report.diagnostics);
    assert_eq!(hits[0].block_id.as_ref(), Some(&bid("html-3")));

    let mut reset = doc();
    reset.blocks.push(table(
        "table-3",
        None,
        vec![ColumnNode::leaf("a", "A")],
        vec![row(&[("a", CellValue::text("x"))])],
    ));
    reset.blocks.push(chart_named(
        "chart-3",
        serde_json::json!({"series": [{"type": "bar", "data": [1]}]}),
    ));
    reset.blocks.push(paragraph(
        "analysis",
        "The table and chart show a 18% increase.",
    ));
    reset.blocks.push(chart_named(
        "chart-4",
        serde_json::json!({"series": [{"type": "bar", "data": [1]}]}),
    ));
    assert!(!has_code(&lint_document(&reset), "QR416"));

    let mut quote_reset = doc();
    quote_reset.blocks.push(table(
        "table-4",
        None,
        vec![ColumnNode::leaf("a", "A")],
        vec![row(&[("a", CellValue::text("x"))])],
    ));
    quote_reset.blocks.push(chart_named(
        "chart-5",
        serde_json::json!({"series": [{"type": "bar", "data": [1]}]}),
    ));
    quote_reset
        .blocks
        .push(Block::quote(bid("quote-reset"), "Analytical takeaway."));
    quote_reset.blocks.push(chart_named(
        "chart-6",
        serde_json::json!({"series": [{"type": "bar", "data": [1]}]}),
    ));
    assert!(!has_code(&lint_document(&quote_reset), "QR416"));
}

#[test]
fn qr420_flags_redundant_note_prefixes() {
    let notes = [
        "注：数据来源为教育部。",
        "注: 数据来源为教育部。",
        "表注：统计口径为2025年。",
        "表注: 统计口径为2025年。",
        "图注：数据来自学校官网。",
        "图注: 数据来自学校官网。",
        "Note: source is the Ministry of Education.",
        "note: source is the Ministry of Education.",
    ];

    let mut flagged = doc();
    for (index, note) in notes.iter().enumerate() {
        let id = format!("note-{index}");
        flagged.blocks.push(with_table_note(
            table(
                id.as_str(),
                None,
                vec![ColumnNode::leaf("a", "A")],
                vec![row(&[("a", CellValue::text("x"))])],
            ),
            note,
        ));
    }
    let report = lint_document(&flagged);
    let hits: Vec<_> = report
        .diagnostics
        .iter()
        .filter(|diagnostic| diagnostic.code == "QR420")
        .collect();
    assert_eq!(hits.len(), notes.len(), "{:?}", report.diagnostics);
    assert!(hits.iter().all(|diagnostic| diagnostic.suppressible
        && diagnostic.message.contains("redundant `注：` prefix")));
    assert_eq!(
        hits.first()
            .and_then(|diagnostic| diagnostic.path.as_deref()),
        Some("blocks[0].note")
    );

    let mut chart = chart_named(
        "chart-note",
        serde_json::json!({"series": [{"type": "bar", "data": [1, 2]}]}),
    );
    if let BlockKind::Echarts(chart_block) = &mut chart.body {
        chart_block.note = Some("图注：数据来自学校官网。".into());
    }
    let mut chart_document = doc();
    chart_document.blocks.push(chart);
    let report = lint_document(&chart_document);
    assert_eq!(
        finding(&report, "QR420").path.as_deref(),
        Some("blocks[0].note")
    );

    let mut clean = doc();
    clean.blocks.push(with_table_note(
        table(
            "clean-note",
            None,
            vec![ColumnNode::leaf("a", "A")],
            vec![row(&[("a", CellValue::text("x"))])],
        ),
        "数据来源：教育部。",
    ));
    let mut html = custom_html("<div>KPI</div>", "KPI");
    if let BlockKind::CustomHtml(custom) = &mut html.body {
        custom.note = Some("注：不计入合计。".into());
    }
    clean.blocks.push(html);
    assert!(!has_code(&lint_document(&clean), "QR420"));

    let mut ignored = with_table_note(
        table(
            "ignored-note",
            None,
            vec![ColumnNode::leaf("a", "A")],
            vec![row(&[("a", CellValue::text("x"))])],
        ),
        "注：可忽略。",
    );
    ignored.lint_ignore = vec!["QR420".into()];
    let mut ignored_document = doc();
    ignored_document.blocks.push(ignored);
    let report = lint_document(&ignored_document);
    assert!(!has_code(&report, "QR420"));
    assert!(report.summary.suppressed >= 1);
}

#[test]
fn qr425_flags_redundant_source_note_prefixes() {
    let prefixed = [
        "资料来源：四校公开年度报告及主管部门公示材料汇总整理。",
        "资料来源: 四校公开年度报告及主管部门公示材料汇总整理。",
        "数据来源：教育部。",
        "数据来源: 教育部。",
        "来源：国家统计局。",
        "信息来源：学校官网。",
        "材料来源：年度质量报告。",
        "数据出处：统计年鉴。",
        "资料来源：来源：教育部。",
        "资料来源：数据来源：四校公开年度报告及主管部门公示材料汇总整理。",
        "数据来源（教育部）：四校公开年度报告。",
        "资料来源(四校年报)：汇总整理。",
        "Source: Ministry of Education.",
        "References: Annual reports.",
    ];

    let mut flagged = doc();
    for (index, source_note) in prefixed.iter().enumerate() {
        let mut block = table(
            &format!("source-prefix-{index}"),
            None,
            vec![ColumnNode::leaf("a", "A")],
            vec![row(&[("a", CellValue::text("x"))])],
        );
        block.source_note = Some((*source_note).into());
        flagged.blocks.push(block);
    }
    let mut chart = chart_named(
        "source-prefix-chart",
        serde_json::json!({"series": [{"type": "bar", "data": [1, 2]}]}),
    );
    chart.source_note = Some("数据来源：教育部。".into());
    flagged.blocks.push(chart);
    let mut html = custom_html("<div>KPI</div>", "KPI");
    html.source_note = Some("来源：学校官网。".into());
    flagged.blocks.push(html);

    let report = lint_document(&flagged);
    let hits: Vec<_> = report
        .diagnostics
        .iter()
        .filter(|diagnostic| diagnostic.code == "QR425")
        .collect();
    assert_eq!(hits.len(), prefixed.len() + 2, "{:?}", report.diagnostics);
    assert!(hits.iter().all(|diagnostic| diagnostic.suppressible
        && diagnostic.severity == Severity::Warning
        && diagnostic.message.contains("redundant `")));
    assert!(
        hits.iter()
            .any(|diagnostic| diagnostic.path.as_deref() == Some("blocks[0].source_note")),
        "{:?}",
        report.diagnostics
    );

    let mut clean = doc();
    for (index, source_note) in [
        "四校公开年度报告及主管部门公示材料汇总整理。",
        "来源与口径：四校公开年度报告。",
        "来源与口径（2025）：四校公开年度报告。",
        "2025年数据来源说明：教育部。",
        "https://example.com/data",
    ]
    .iter()
    .enumerate()
    {
        let mut block = table(
            &format!("clean-source-{index}"),
            None,
            vec![ColumnNode::leaf("a", "A")],
            vec![row(&[("a", CellValue::text("x"))])],
        );
        block.source_note = Some((*source_note).into());
        clean.blocks.push(block);
    }
    let mut paragraph = paragraph("paragraph-source", "正文");
    paragraph.source_note = Some("资料来源：教育部。".into());
    clean.blocks.push(paragraph);
    assert!(!has_code(&lint_document(&clean), "QR425"));

    let mut ignored = table(
        "ignored-source",
        None,
        vec![ColumnNode::leaf("a", "A")],
        vec![row(&[("a", CellValue::text("x"))])],
    );
    ignored.source_note = Some("资料来源：教育部。".into());
    ignored.lint_ignore = vec!["QR425".into()];
    let mut ignored_document = doc();
    ignored_document.blocks.push(ignored);
    let report = lint_document(&ignored_document);
    assert!(!has_code(&report, "QR425"));
    assert!(report.summary.suppressed >= 1);
}

#[test]
fn pending_heading_is_qr401_not_qr418() {
    let mut pending = doc();
    pending.blocks.push(heading(
        "pending-heading",
        2,
        "[待拟标题：国自然立项体量与结构对标]",
    ));
    let report = lint_document(&pending);
    assert!(
        has_code(&report, "QR401"),
        "pending headings must be blocked"
    );
    assert!(
        !has_code(&report, "QR418"),
        "a concrete placeholder title is not a generic counting heading"
    );
}

#[test]
fn qr418_flags_generic_and_counting_headings() {
    let mut generic = doc();
    generic
        .blocks
        .push(heading("generic-zh", 2, "一、总结与展望"));
    assert!(has_code(&lint_document(&generic), "QR418"));

    let mut english = doc();
    english.blocks.push(heading("generic-en", 2, "Overview"));
    assert!(has_code(&lint_document(&english), "QR418"));

    let mut counting = doc();
    counting.blocks.push(heading("counting", 2, "三大核心维度"));
    assert!(has_code(&lint_document(&counting), "QR418"));

    let mut action_advice = doc();
    action_advice
        .blocks
        .push(heading("action-advice", 2, "八、行动建议"));
    assert!(has_code(&lint_document(&action_advice), "QR418"));

    let mut h1 = doc();
    h1.blocks.push(heading("h1-generic", 1, "概述"));
    assert!(!has_code(&lint_document(&h1), "QR418"));

    let mut concrete = doc();
    concrete
        .blocks
        .push(heading("concrete", 2, "三大驱动力：政策、算力与场景"));
    assert!(
        !has_code(&lint_document(&concrete), "QR418"),
        "a counting heading with a concrete colon suffix must be exempt"
    );

    let mut decimal = doc();
    decimal
        .blocks
        .push(heading("decimal-percent", 2, "3.5% 增速背后的结构性因素"));
    assert!(
        !has_code(&lint_document(&decimal), "QR418"),
        "a decimal percentage must not be treated as heading numbering"
    );
}

#[test]
fn qr418_flags_standalone_scope_sections_in_any_position() {
    for text in [
        "数据口径与说明",
        "口径与说明",
        "统计口径说明",
        "数据说明",
        "统计范围与时间窗口说明",
        "指标释义",
    ] {
        let mut document = doc();
        document.blocks.push(heading("scope-only", 2, text));
        let report = lint_document(&document);
        assert!(
            has_code(&report, "QR418"),
            "scope heading must report: {text}"
        );
        assert_eq!(
            finding(&report, "QR418").block_id.as_ref(),
            Some(&bid("scope-only"))
        );
    }

    let mut concrete = doc();
    concrete.blocks.push(heading(
        "concrete-scope-heading",
        2,
        "数据说明：6个A类学科仅1个入围",
    ));
    assert!(!has_code(&lint_document(&concrete), "QR418"));

    let mut after_closing = doc();
    after_closing.blocks.push(heading("closing", 2, "结语"));
    after_closing
        .blocks
        .push(heading("scope-after-closing", 2, "数据口径与说明"));
    let report = lint_document(&after_closing);
    assert!(
        report.diagnostics.iter().any(|diagnostic| {
            diagnostic.code == "QR418"
                && diagnostic.block_id.as_ref() == Some(&bid("scope-after-closing"))
        }),
        "a scope chapter after the conclusion must never inherit a closing exemption: {:?}",
        report.diagnostics
    );
}

#[test]
fn qr418_allows_positional_introduction_and_conclusion_headings() {
    let mut document = doc();
    document.blocks.push(heading("intro", 2, "一、引言"));
    document
        .blocks
        .push(paragraph("intro-body", "说明报告背景与分析对象。"));
    document.blocks.push(heading("analysis", 2, "二、现状分析"));
    document
        .blocks
        .push(paragraph("analysis-body", "分析具体指标与结构。"));
    document.blocks.push(heading("ending", 2, "三、结语"));
    document
        .blocks
        .push(paragraph("ending-body", "回扣核心判断与建设方向。"));

    let report = lint_document(&document);
    let hits: Vec<_> = report
        .diagnostics
        .iter()
        .filter(|diagnostic| diagnostic.code == "QR418")
        .collect();
    assert_eq!(hits.len(), 1, "{:?}", report.diagnostics);
    assert_eq!(hits[0].block_id.as_ref(), Some(&bid("analysis")));
    assert!(hits[0].hint.contains("keep its column label"));

    let mut nested = doc();
    nested.blocks.push(heading("nested-ending", 3, "结语"));
    assert!(
        has_code(&lint_document(&nested), "QR418"),
        "the positional exemption applies only to the final H2, not arbitrary nested headings"
    );
}

#[test]
fn qr407_ignores_currency_amounts() {
    let mut document = doc();
    document
        .blocks
        .push(paragraph("budget", "经费 $500 万，另有 $12.5M 的预算。"));
    let report = lint_document(&document);
    assert!(
        !has_code(&report, "QR407"),
        "currency amounts must not be treated as math delimiters: {:?}",
        report.diagnostics
    );
}

#[test]
fn qr407_detects_unpaired_math_delimiter() {
    let mut document = doc();
    document
        .blocks
        .push(paragraph("formula", "公式 $x + y 尚未闭合。"));
    let report = lint_document(&document);
    assert!(has_code(&report, "QR407"), "{:?}", report.diagnostics);
}

#[test]
fn qr418_flags_structural_judgment_and_action_line_counting_headings() {
    for raw in ["三条结构性判断", "四条主攻线", "五大阶段性任务"] {
        let mut document = doc();
        document
            .blocks
            .push(heading("h1", 1, "总体态势：海洋学科领跑"));
        document.blocks.push(heading("h2", 2, raw));
        document
            .blocks
            .push(paragraph("p1", "2025年经费达1.2亿元。"));
        let report = lint_document(&document);
        assert!(
            has_code(&report, "QR418"),
            "bare counting heading `{raw}` must trigger QR418: {:?}",
            report.diagnostics
        );
    }

    let mut concrete = doc();
    concrete
        .blocks
        .push(heading("h1", 1, "总体态势：海洋学科领跑"));
    concrete.blocks.push(heading(
        "h2",
        2,
        "三条结构性判断：海洋水产领跑、工科增量补位、高层次人才待破局",
    ));
    concrete
        .blocks
        .push(paragraph("p1", "2025年经费达1.2亿元。"));
    assert!(!has_code(&lint_document(&concrete), "QR418"));
}
