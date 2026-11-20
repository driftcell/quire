#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::indexing_slicing,
    clippy::panic
)]

use std::collections::BTreeMap;

use quire_lint::{
    LintOptions, lint_document, lint_document_with_options, lint_echarts_block,
    normalize::normalize_block,
};
use quire_protocol::{
    Block, BlockId, BlockKind, BlockStyle, CellValue, ColumnNode, CustomHtmlBlock, DocId,
    EchartsBlock, LintConfig, LintReport, ReportDocument, Severity, SourceEntry, SourceId,
    TableBlock,
};

fn doc() -> ReportDocument {
    ReportDocument::new(
        DocId::parse("lint-doc").unwrap_or_else(|error| panic!("valid lint doc id: {error}")),
    )
}

fn bid(raw: &str) -> BlockId {
    BlockId::parse(raw).unwrap_or_else(|error| panic!("valid block id `{raw}`: {error}"))
}

fn sid(raw: &str) -> SourceId {
    SourceId::parse(raw).unwrap_or_else(|error| panic!("valid source id `{raw}`: {error}"))
}

fn paragraph(id: &str, text: &str) -> Block {
    Block::paragraph(bid(id), text)
}

fn heading(id: &str, level: u8, text: &str) -> Block {
    Block::heading(bid(id), level, text)
}

fn table(
    id: &str,
    title: Option<&str>,
    columns: Vec<ColumnNode>,
    rows: Vec<BTreeMap<String, CellValue>>,
) -> Block {
    Block::table(
        bid(id),
        TableBlock {
            title: title.map(str::to_string),
            numbering: None,
            note: None,
            columns,
            rows,
            summary_rows: Vec::new(),
        },
    )
}

fn row(pairs: &[(&str, CellValue)]) -> BTreeMap<String, CellValue> {
    pairs
        .iter()
        .map(|(key, value)| ((*key).to_string(), value.clone()))
        .collect()
}

fn valid_document() -> ReportDocument {
    let mut document = doc();
    document
        .sources
        .insert(sid("sec10"), SourceEntry::new("Annual filing"));
    document.blocks.push(heading("intro", 1, "Introduction"));
    document
        .blocks
        .push(paragraph("summary", "Revenue grew 18% [^sec10]."));
    document.blocks.push(table(
        "table",
        None,
        vec![
            ColumnNode::leaf("region", "Region"),
            ColumnNode::leaf("value", "Value"),
        ],
        vec![
            row(&[
                ("region", CellValue::text("APAC")),
                ("value", CellValue::Integer(10)),
            ]),
            row(&[
                ("region", CellValue::text("EMEA")),
                ("value", CellValue::Integer(20)),
            ]),
        ],
    ));
    document
}

fn codes(report: &LintReport) -> Vec<&str> {
    report
        .diagnostics
        .iter()
        .map(|diagnostic| diagnostic.code.as_str())
        .collect()
}

fn has_code(report: &LintReport, code: &str) -> bool {
    codes(report).contains(&code)
}

fn finding<'a>(report: &'a LintReport, code: &str) -> &'a quire_protocol::Diagnostic {
    report
        .diagnostics
        .iter()
        .find(|diagnostic| diagnostic.code == code)
        .unwrap_or_else(|| panic!("missing {code}: {:?}", report.diagnostics))
}

fn chart(option: serde_json::Value) -> Block {
    chart_named("chart", option)
}

fn chart_named(id: &str, option: serde_json::Value) -> Block {
    Block::new(
        bid(id),
        BlockKind::Echarts(EchartsBlock {
            title: Some("Revenue".into()),
            height: 320,
            option,
            option_patch: None,
            fallback_summary: "Revenue rose.".into(),
            numbering: None,
            note: None,
        }),
    )
}

fn custom_html(html: &str, fallback: &str) -> Block {
    Block::new(
        bid("html"),
        BlockKind::CustomHtml(CustomHtmlBlock {
            title: Some("KPI Grid".into()),
            numbering: None,
            note: None,
            html: html.into(),
            css: None,
            fallback_text: fallback.into(),
        }),
    )
}

#[test]
fn valid_document_is_clean() {
    let report = lint_document(&valid_document());
    assert!(report.is_clean(), "{:?}", report.diagnostics);
    assert_eq!(report.summary.suppressed, 0);
}

#[test]
fn core_contract_rules_are_detected() {
    let mut duplicate = doc();
    duplicate.blocks.push(paragraph("same", "a"));
    duplicate.blocks.push(paragraph("same", "b"));
    assert!(has_code(&lint_document(&duplicate), "QR001"));

    let mut uuid = doc();
    uuid.blocks.push(Block::paragraph(
        bid("123e4567-e89b-42d3-a456-426614174000"),
        "x",
    ));
    assert!(has_code(&lint_document(&uuid), "QR003"));

    let mut level = doc();
    level.blocks.push(heading("bad", 9, "Too deep"));
    assert!(has_code(&lint_document(&level), "QR004"));

    let mut schema = doc();
    schema.blocks.push(table(
        "bad-table",
        None,
        vec![
            ColumnNode::leaf("a", "A"),
            ColumnNode::leaf("a", "Duplicate"),
        ],
        vec![],
    ));
    assert!(has_code(&lint_document(&schema), "QR005"));

    let mut unknown = doc();
    unknown.blocks.push(table(
        "unknown",
        None,
        vec![ColumnNode::leaf("a", "A")],
        vec![row(&[("b", CellValue::text("x"))])],
    ));
    let report = lint_document(&unknown);
    assert_eq!(finding(&report, "QR006").severity, Severity::Warning);
    assert!(!finding(&report, "QR006").suppressible);

    let mut widths = doc();
    widths.blocks.push(
        Block::table(
            bid("widths"),
            TableBlock {
                title: None,
                numbering: None,
                note: None,
                columns: vec![ColumnNode::leaf("a", "A")],
                rows: vec![row(&[("a", CellValue::text("x"))])],
                summary_rows: Vec::new(),
            },
        )
        .with_style(BlockStyle {
            column_widths: Some(BTreeMap::from([("ghost".into(), "10%".into())])),
            ..BlockStyle::default()
        }),
    );
    assert!(has_code(&lint_document(&widths), "QR007"));
}

#[test]
fn citation_and_source_rules_are_detected() {
    let mut dangling = doc();
    dangling
        .blocks
        .push(paragraph("bad", "Revenue [^missing]."));
    let report = lint_document(&dangling);
    let diagnostic = finding(&report, "QR101");
    assert_eq!(diagnostic.severity, Severity::Error);
    assert!(!diagnostic.suppressible);
    assert_eq!(diagnostic.rule, "dangling-source-id");

    let mut unused = doc();
    unused
        .sources
        .insert(sid("unused"), SourceEntry::new("Unused"));
    unused.blocks.push(paragraph("body", "No citations here."));
    assert!(has_code(&lint_document(&unused), "QR102"));

    let mut numeric = doc();
    numeric
        .blocks
        .push(paragraph("claim", "Revenue grew 18% year over year."));
    let report = lint_document(&numeric);
    assert_eq!(finding(&report, "QR103").severity, Severity::Info);

    let mut duplicate = doc();
    duplicate.sources.insert(
        sid("one"),
        SourceEntry {
            url: Some("https://example.com/data".into()),
            ..SourceEntry::new("One")
        },
    );
    duplicate.sources.insert(
        sid("two"),
        SourceEntry {
            url: Some("https://example.com/data/".into()),
            ..SourceEntry::new("Two")
        },
    );
    duplicate
        .blocks
        .push(paragraph("cited", "Evidence [^one] and [^two]."));
    assert!(has_code(&lint_document(&duplicate), "QR104"));

    let mut empty_title = doc();
    empty_title
        .sources
        .insert(sid("empty"), SourceEntry::new(""));
    empty_title.blocks.push(paragraph("body", "Text [^empty]."));
    assert!(has_code(&lint_document(&empty_title), "QR105"));

    let mut unsafe_url = doc();
    unsafe_url.sources.insert(
        sid("unsafe"),
        SourceEntry {
            url: Some("javascript:alert(1)".into()),
            ..SourceEntry::new("Unsafe")
        },
    );
    unsafe_url.blocks.push(paragraph("body", "Text [^unsafe]."));
    assert!(has_code(&lint_document(&unsafe_url), "QR106"));
}

#[test]
fn malformed_citation_syntax_is_rejected() {
    let mut document = doc();
    document
        .sources
        .insert(sid("jxufe_101"), SourceEntry::new("JXUFE 101"));
    document
        .sources
        .insert(sid("jxufe_102"), SourceEntry::new("JXUFE 102"));
    document.blocks.push(paragraph(
        "colon",
        "招生计划 [^jxufe_101: 【2025招生】计划] 已公布。",
    ));

    let report = lint_document(&document);
    let diagnostic = finding(&report, "QR107");
    assert_eq!(diagnostic.severity, Severity::Error);
    assert_eq!(diagnostic.rule, "malformed-citation-syntax");
    assert!(!diagnostic.suppressible);
    assert!(
        diagnostic.hint.contains("[^jxufe_101]"),
        "{}",
        diagnostic.hint
    );
}

#[test]
fn malformed_citation_separators_and_whitespace_are_rejected() {
    for text in [
        "[^jxufe_101, jxufe_102]",
        "[^jxufe_101; jxufe_102]",
        "[^jxufe_101、jxufe_102]",
    ] {
        let mut document = doc();
        document
            .sources
            .insert(sid("jxufe_101"), SourceEntry::new("JXUFE 101"));
        document
            .sources
            .insert(sid("jxufe_102"), SourceEntry::new("JXUFE 102"));
        document
            .blocks
            .push(paragraph("merged", &format!("数据 {text}。")));

        let report = lint_document(&document);
        let diagnostic = finding(&report, "QR107");
        assert!(
            diagnostic.hint.contains("[^jxufe_101][^jxufe_102]"),
            "{}",
            diagnostic.hint
        );
    }

    for text in ["[^ jxufe_101]", "[^jxufe_101 ]", "[^jxufe 101]", "[^]"] {
        let mut document = doc();
        document
            .sources
            .insert(sid("jxufe_101"), SourceEntry::new("JXUFE 101"));
        document
            .blocks
            .push(paragraph("space", &format!("数据 {text}。")));

        let report = lint_document(&document);
        let diagnostic = finding(&report, "QR107");
        assert!(
            diagnostic.hint.contains("[^jxufe_101]"),
            "{}",
            diagnostic.hint
        );
    }
}

#[test]
fn malformed_citation_before_another_marker_is_not_skipped() {
    let mut document = doc();
    document
        .sources
        .insert(sid("jxufe_101"), SourceEntry::new("JXUFE 101"));
    document
        .sources
        .insert(sid("jxufe_102"), SourceEntry::new("JXUFE 102"));
    document.blocks.push(paragraph(
        "adjacent",
        "数据 [^jxufe_101: 标题][^jxufe_102]。",
    ));

    let report = lint_document(&document);
    assert!(has_code(&report, "QR107"), "{:?}", report.diagnostics);
}

#[test]
fn registered_source_id_without_caret_is_rejected() {
    let mut document = doc();
    document
        .sources
        .insert(sid("jxufe_101"), SourceEntry::new("JXUFE 101"));
    document
        .blocks
        .push(paragraph("missing-caret", "见 [jxufe_101]。"));

    let report = lint_document(&document);
    let diagnostic = finding(&report, "QR107");
    assert!(
        diagnostic.hint.contains("[^jxufe_101]"),
        "{}",
        diagnostic.hint
    );
}

#[test]
fn valid_citations_and_links_do_not_trigger_malformed_syntax() {
    let mut document = doc();
    document
        .sources
        .insert(sid("jxufe_101"), SourceEntry::new("JXUFE 101"));
    document
        .sources
        .insert(sid("jxufe_102"), SourceEntry::new("JXUFE 102"));
    document.blocks.push(paragraph(
        "valid",
        "连写 [^jxufe_101][^jxufe_102]，普通括号 [注1]，链接 [jxufe_101](https://example.com)，引用链接 [jxufe_101][ref]。",
    ));

    let report = lint_document(&document);
    assert!(!has_code(&report, "QR107"), "{:?}", report.diagnostics);
}

#[test]
fn malformed_citations_are_checked_in_notes_and_source_notes() {
    let mut document = doc();
    document
        .sources
        .insert(sid("jxufe_101"), SourceEntry::new("JXUFE 101"));

    let mut table_block = table(
        "note-table",
        None,
        vec![ColumnNode::leaf("a", "A")],
        vec![row(&[("a", CellValue::text("x"))])],
    );
    if let BlockKind::Table(ref mut table) = table_block.body {
        table.note = Some("口径见 [^jxufe_101: 公报]".into());
    }
    table_block.source_note = Some("来源 [jxufe_101]".into());
    document.blocks.push(table_block);

    let mut chart_block = chart_named(
        "note-chart",
        serde_json::json!({"series": [{"type": "bar", "data": [1]}]}),
    );
    if let BlockKind::Echarts(ref mut chart) = chart_block.body {
        chart.note = Some("说明 [^jxufe_101, jxufe_102]".into());
    }
    document.blocks.push(chart_block);

    let mut html_block = custom_html("<div>KPI</div>", "KPI");
    if let BlockKind::CustomHtml(ref mut html) = html_block.body {
        html.note = Some("注释 [^jxufe_101 ]".into());
    }
    document.blocks.push(html_block);

    let report = lint_document(&document);
    let hits: Vec<_> = report
        .diagnostics
        .iter()
        .filter(|diagnostic| diagnostic.code == "QR107")
        .collect();
    assert_eq!(hits.len(), 4, "{:?}", report.diagnostics);
    let paths: Vec<_> = hits
        .iter()
        .map(|diagnostic| diagnostic.path.as_deref().unwrap_or(""))
        .collect();
    assert!(paths.contains(&"blocks[0].note"));
    assert!(paths.contains(&"blocks[0].source_note"));
    assert!(paths.contains(&"blocks[1].note"));
    assert!(paths.contains(&"blocks[2].note"));
}

#[test]
fn table_layout_rules_are_detected() {
    let mut narrow = doc();
    narrow.blocks.push(
        table(
            "narrow",
            None,
            vec![
                ColumnNode::leaf("metric", "指标名称"),
                ColumnNode::leaf("region", "地区"),
                ColumnNode::leaf("note", "备注"),
            ],
            vec![
                row(&[
                    ("metric", CellValue::text("营收")),
                    ("region", CellValue::text("华东")),
                    ("note", CellValue::text("A")),
                ]),
                row(&[
                    ("metric", CellValue::text("利润")),
                    ("region", CellValue::text("华南")),
                    ("note", CellValue::text("B")),
                ]),
            ],
        )
        .with_style(BlockStyle {
            column_widths: Some(BTreeMap::from([("metric".into(), "40px".into())])),
            ..BlockStyle::default()
        }),
    );
    assert!(has_code(&lint_document(&narrow), "QR201"));

    let mut empty = doc();
    empty.blocks.push(table(
        "empty",
        None,
        vec![ColumnNode::leaf("a", "A"), ColumnNode::leaf("b", "B")],
        vec![
            row(&[("a", CellValue::text("x")), ("b", CellValue::Null)]),
            row(&[("a", CellValue::text("y")), ("b", CellValue::Null)]),
        ],
    ));
    assert!(has_code(&lint_document(&empty), "QR202"));

    let mut constant = doc();
    constant.blocks.push(table(
        "constant",
        None,
        vec![ColumnNode::leaf("a", "A"), ColumnNode::leaf("b", "B")],
        vec![
            row(&[("a", CellValue::text("x")), ("b", CellValue::text("same"))]),
            row(&[("a", CellValue::text("y")), ("b", CellValue::text("same"))]),
            row(&[("a", CellValue::text("z")), ("b", CellValue::text("same"))]),
        ],
    ));
    assert!(has_code(&lint_document(&constant), "QR203"));

    let mut wide = doc();
    let columns: Vec<_> = (0..8)
        .map(|index| ColumnNode::leaf(format!("c{index}"), "指标名称列甲乙"))
        .collect();
    let wide_row: BTreeMap<String, CellValue> = (0..8)
        .map(|index| (format!("c{index}"), CellValue::text("数据")))
        .collect();
    wide.blocks.push(table(
        "wide",
        None,
        columns,
        vec![wide_row.clone(), wide_row],
    ));
    assert!(has_code(&lint_document(&wide), "QR204"));
    assert!(has_code(&lint_document(&wide), "QR201"));

    let mut units = doc();
    units.blocks.push(table(
        "units",
        None,
        vec![
            ColumnNode::leaf("rate", "增长率"),
            ColumnNode::leaf("name", "名称"),
        ],
        vec![
            row(&[
                ("rate", CellValue::text("12%")),
                ("name", CellValue::text("A")),
            ]),
            row(&[
                ("rate", CellValue::text("5")),
                ("name", CellValue::text("B")),
            ]),
            row(&[
                ("rate", CellValue::text("8%")),
                ("name", CellValue::text("C")),
            ]),
        ],
    ));
    assert!(has_code(&lint_document(&units), "QR205"));
}

#[test]
fn chart_safety_rules_are_detected() {
    let mut missing = doc();
    missing.blocks.push(chart(serde_json::json!({
        "series": [{"type": "bar", "data": []}]
    })));
    let report = lint_document(&missing);
    let diagnostic = finding(&report, "QR301");
    assert!(!diagnostic.suppressible);

    let mut js = doc();
    js.blocks.push(chart(serde_json::json!({
        "tooltip": {"formatter": "function(p) { return p.name; }"},
        "series": [{"type": "bar", "data": [1]}]
    })));
    assert!(!finding(&lint_document(&js), "QR302").suppressible);

    let mut invalid = doc();
    invalid.blocks.push(chart(serde_json::json!({
        "series": [{"type": "notARealType", "data": [1]}]
    })));
    assert!(has_code(&lint_document(&invalid), "QR305"));

    let isolated = lint_echarts_block(
        bid("preview"),
        &EchartsBlock {
            title: None,
            height: 300,
            option: serde_json::json!({"series": [{"type": "bar", "data": [1]}]}),
            option_patch: None,
            fallback_summary: "One bar.".into(),
            numbering: None,
            note: None,
        },
    );
    assert!(isolated.is_clean(), "{:?}", isolated.diagnostics);
}

#[test]
fn echarts_render_blockers_cannot_be_ignored() {
    let mut document = doc();
    let mut missing = chart_named("missing-option", serde_json::Value::Null);
    missing.lint_ignore = vec!["QR303".into(), "QR3".into()];
    let mut not_object = chart_named("not-an-object", serde_json::json!("invalid"));
    not_object.lint_ignore = vec!["QR304".into(), "QR3".into()];
    document.blocks.push(missing);
    document.blocks.push(not_object);
    document.lint.ignore = vec!["QR303".into(), "QR304".into()];

    let report = lint_document_with_options(
        &document,
        &LintOptions {
            select: Vec::new(),
            ignore: vec!["QR3".into(), "QR303".into(), "QR304".into()],
        },
    );
    assert!(has_code(&report, "QR303"), "{:?}", report.diagnostics);
    assert!(has_code(&report, "QR304"), "{:?}", report.diagnostics);
    assert!(!finding(&report, "QR303").suppressible);
    assert!(!finding(&report, "QR304").suppressible);
    assert_eq!(report.summary.suppressed, 0);
}

#[test]
fn echarts_entity_color_mismatch_is_reported_across_charts() {
    let mut document = doc();
    document.blocks.push(chart_named(
        "chart_1",
        serde_json::json!({
            "series": [
                {"name": "浙江理工大学", "type": "bar", "data": [1]}
            ]
        }),
    ));
    document.blocks.push(chart_named(
        "chart_2",
        serde_json::json!({
            "series": [
                {"name": "样本高校", "type": "bar", "data": [2]},
                {"name": "浙江理工大学", "type": "bar", "data": [3]}
            ]
        }),
    ));

    let report = lint_document(&document);
    let diagnostic = finding(&report, "QR314");
    assert_eq!(diagnostic.severity, Severity::Warning);
    assert!(diagnostic.suppressible);
    assert_eq!(diagnostic.block_id.as_ref(), Some(&bid("chart_2")));
    assert!(diagnostic.hint.contains("浙江理工大学"));
    assert!(diagnostic.hint.contains("#5070dd"));
    assert!(diagnostic.hint.contains("#b6d634"));
    assert!(diagnostic.hint.contains("chart_1"));
    assert!(diagnostic.hint.contains("chart_2"));
}

#[test]
fn echarts_fixed_entity_colors_ignore_series_order() {
    let mut document = doc();
    document.blocks.push(chart_named(
        "chart_1",
        serde_json::json!({
            "series": [{
                "name": "浙江理工大学",
                "type": "bar",
                "itemStyle": {"color": "#1E3A5F"},
                "data": [1]
            }]
        }),
    ));
    document.blocks.push(chart_named(
        "chart_2",
        serde_json::json!({
            "series": [
                {"name": "样本高校", "type": "bar", "data": [2]},
                {
                    "name": "浙江理工大学",
                    "type": "bar",
                    "itemStyle": {"color": "#1e3a5f"},
                    "data": [3]
                }
            ]
        }),
    ));

    let report = lint_document(&document);
    assert!(!has_code(&report, "QR314"), "{:?}", report.diagnostics);
}

#[test]
fn echarts_year_and_generic_labels_do_not_trigger_color_mismatch() {
    let mut document = doc();
    document.blocks.push(chart_named(
        "chart_1",
        serde_json::json!({
            "series": [{
                "type": "pie",
                "data": [
                    {"name": "2025", "value": 1},
                    {"name": "平均值", "value": 2}
                ]
            }]
        }),
    ));
    document.blocks.push(chart_named(
        "chart_2",
        serde_json::json!({
            "series": [{
                "type": "pie",
                "data": [
                    {"name": "样本高校", "value": 3},
                    {"name": "2025", "value": 1},
                    {"name": "平均值", "value": 2}
                ]
            }]
        }),
    ));

    let report = lint_document(&document);
    assert!(!has_code(&report, "QR314"), "{:?}", report.diagnostics);
}

#[test]
fn echarts_explicit_palettes_must_match_across_charts() {
    let mut document = doc();
    document.blocks.push(chart_named(
        "chart_1",
        serde_json::json!({
            "color": ["#111111", "#222222"],
            "series": [{"name": "浙江理工大学", "type": "bar", "data": [1]}]
        }),
    ));
    document.blocks.push(chart_named(
        "chart_2",
        serde_json::json!({
            "color": ["#111111", "#333333"],
            "series": [{"name": "浙江理工大学", "type": "bar", "data": [2]}]
        }),
    ));

    let report = lint_document(&document);
    let diagnostic = finding(&report, "QR315");
    assert_eq!(diagnostic.severity, Severity::Warning);
    assert!(diagnostic.suppressible);
    assert_eq!(diagnostic.block_id.as_ref(), Some(&bid("chart_2")));
    assert!(diagnostic.hint.contains("option.color"));
}

#[test]
fn cross_chart_color_rules_can_be_block_ignored() {
    let mut document = doc();
    document.blocks.push(chart_named(
        "chart_1",
        serde_json::json!({
            "color": ["#111111", "#222222"],
            "series": [{"name": "浙江理工大学", "type": "bar", "data": [1]}]
        }),
    ));
    let mut second = chart_named(
        "chart_2",
        serde_json::json!({
            "color": ["#333333", "#444444"],
            "series": [{"name": "浙江理工大学", "type": "bar", "data": [2]}]
        }),
    );
    second.lint_ignore = vec!["QR314".into(), "QR315".into()];
    document.blocks.push(second);

    let report = lint_document(&document);
    assert!(!has_code(&report, "QR314"), "{:?}", report.diagnostics);
    assert!(!has_code(&report, "QR315"), "{:?}", report.diagnostics);
    assert!(report.summary.suppressed >= 2, "{:?}", report.summary);
}

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
    assert!(has_code(&lint_document(&sequence), "QR405"));

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
fn qr412_detects_strong_ai_cliches() {
    for (id, text, phrase) in [
        ("summary", "综上所述，项目进展顺利。", "综上所述"),
        ("noting", "值得注意的是，数据仍有缺口。", "值得注意的是"),
    ] {
        let mut document = doc();
        document.blocks.push(paragraph(id, text));
        let report = lint_document(&document);
        let diagnostic = finding(&report, "QR412");
        assert_eq!(diagnostic.severity, Severity::Warning);
        assert!(diagnostic.suppressible);
        assert!(
            diagnostic.message.contains(phrase),
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
        ("spearman", "采用Spearman相关系数分析。", "Spearman相关系数"),
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
    }
}

#[test]
fn qr412_counts_new_b_class_words_by_distinct_density() {
    let mut single = doc();
    single
        .blocks
        .push(paragraph("single", "显著提升是当前的主要判断。"));
    assert!(!has_code(&lint_document(&single), "QR412"));

    let mut pair = doc();
    pair.blocks.push(paragraph("pair", "显著提升、稳中有进。"));
    assert!(has_code(&lint_document(&pair), "QR412"));

    let mut transition_pair = doc();
    transition_pair
        .blocks
        .push(paragraph("transition-pair", "与此同时，总体平稳。"));
    assert!(has_code(&lint_document(&transition_pair), "QR412"));

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
fn qr412_requires_two_distinct_jargon_words() {
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
fn qr316_flags_dense_pie_and_category_axis_labels() {
    let pie_eight = (0..8)
        .map(|index| serde_json::json!({"name": format!("S{index}"), "value": index + 1}))
        .collect::<Vec<_>>();
    let mut pie = doc();
    pie.blocks.push(chart_named(
        "pie-8",
        serde_json::json!({"series": [{"type": "pie", "data": pie_eight}]}),
    ));
    let report = lint_document(&pie);
    let diagnostic = finding(&report, "QR316");
    assert_eq!(diagnostic.severity, Severity::Warning);
    assert!(diagnostic.suppressible);
    assert!(
        diagnostic.message.contains("8 slices"),
        "{}",
        diagnostic.message
    );
    assert_eq!(
        diagnostic.path.as_deref(),
        Some("blocks[0].option.series[0].data")
    );

    let pie_seven = (0..7)
        .map(|index| serde_json::json!({"name": format!("S{index}"), "value": index + 1}))
        .collect::<Vec<_>>();
    let mut seven = doc();
    seven.blocks.push(chart_named(
        "pie-7",
        serde_json::json!({"series": [{"type": "pie", "data": pie_seven}]}),
    ));
    assert!(!has_code(&lint_document(&seven), "QR316"));

    let labels: Vec<String> = (0..12).map(|index| format!("类别{index}")).collect();
    let mut dense_axis = doc();
    dense_axis.blocks.push(chart_named(
        "dense-axis",
        serde_json::json!({
            "xAxis": {"type": "category", "data": labels},
            "yAxis": {"type": "value"},
            "series": [{"type": "bar", "data": vec![1; 12]}]
        }),
    ));
    let report = lint_document(&dense_axis);
    let diagnostic = finding(&report, "QR316");
    assert!(diagnostic.message.contains("12 category labels"));
    assert_eq!(diagnostic.path.as_deref(), Some("blocks[0].option.xAxis"));

    let labels: Vec<String> = (0..12).map(|index| format!("类别{index}")).collect();
    let rotated = chart_named(
        "rotated",
        serde_json::json!({
            "xAxis": {
                "type": "category",
                "data": labels,
                "axisLabel": {"rotate": 30}
            },
            "yAxis": {"type": "value"},
            "series": [{"type": "bar", "data": vec![1; 12]}]
        }),
    );
    let mut rotated_doc = doc();
    rotated_doc.blocks.push(rotated);
    assert!(!has_code(&lint_document(&rotated_doc), "QR316"));

    let labels: Vec<String> = (0..12).map(|index| format!("类别{index}")).collect();
    let interval = chart_named(
        "interval",
        serde_json::json!({
            "xAxis": {
                "type": "category",
                "data": labels,
                "axisLabel": {"interval": 1}
            },
            "yAxis": {"type": "value"},
            "series": [{"type": "bar", "data": vec![1; 12]}]
        }),
    );
    let mut interval_doc = doc();
    interval_doc.blocks.push(interval);
    assert!(!has_code(&lint_document(&interval_doc), "QR316"));

    let labels: Vec<String> = (0..15).map(|index| format!("类别{index}")).collect();
    let horizontal = chart_named(
        "horizontal",
        serde_json::json!({
            "xAxis": {"type": "value"},
            "yAxis": {"type": "category", "data": labels},
            "series": [{"type": "bar", "data": vec![1; 15]}]
        }),
    );
    let mut horizontal_doc = doc();
    horizontal_doc.blocks.push(horizontal);
    assert!(
        !has_code(&lint_document(&horizontal_doc), "QR316"),
        "yAxis category labels belong to horizontal bars and must not be checked"
    );

    let mut ignored = chart_named(
        "ignored-pie",
        serde_json::json!({"series": [{"type": "pie", "data": (0..8).map(|index| serde_json::json!({"name": format!("S{index}"), "value": index + 1})).collect::<Vec<_>>()}]}),
    );
    ignored.lint_ignore = vec!["QR316".into()];
    let mut ignored_doc = doc();
    ignored_doc.blocks.push(ignored);
    let report = lint_document(&ignored_doc);
    assert!(!has_code(&report, "QR316"));
    assert!(report.summary.suppressed >= 1);
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
fn qr417_flags_walls_of_text_and_long_prose_runs() {
    let wall = "数".repeat(450);
    let mut single = doc();
    single.blocks.push(paragraph("wall", wall.as_str()));
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
    let paragraph_text = "数".repeat(200);
    for index in 1..=5 {
        let id = format!("run-{index}");
        run.blocks
            .push(paragraph(id.as_str(), paragraph_text.as_str()));
    }
    let report = lint_document(&run);
    let hits: Vec<_> = report
        .diagnostics
        .iter()
        .filter(|diagnostic| diagnostic.code == "QR417")
        .collect();
    assert_eq!(hits.len(), 1, "{:?}", report.diagnostics);
    assert_eq!(hits[0].block_id.as_ref(), Some(&bid("run-5")));
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
fn qr316_qr416_qr417_qr418_can_be_ignored_at_block_and_document_level() {
    let mut document = doc();
    let mut dense = chart_named(
        "dense",
        serde_json::json!({"series": [{"type": "pie", "data": (0..8).map(|index| serde_json::json!({"name": format!("S{index}"), "value": index + 1})).collect::<Vec<_>>()}]}),
    );
    dense.lint_ignore = vec!["QR316".into()];
    document.blocks.push(dense);
    let mut visual_a = table(
        "visual-a",
        None,
        vec![ColumnNode::leaf("a", "A")],
        vec![row(&[("a", CellValue::text("x"))])],
    );
    visual_a.lint_ignore = vec!["QR416".into()];
    document.blocks.push(visual_a);
    let mut visual_b = chart_named(
        "visual-b",
        serde_json::json!({"series": [{"type": "bar", "data": [1]}]}),
    );
    visual_b.lint_ignore = vec!["QR416".into()];
    document.blocks.push(visual_b);
    let mut visual_c = custom_html("<div>KPI</div>", "KPI");
    visual_c.id = bid("visual-c");
    visual_c.lint_ignore = vec!["QR416".into()];
    document.blocks.push(visual_c);
    let mut wall = paragraph("wall", "数".repeat(450).as_str());
    wall.lint_ignore = vec!["QR417".into()];
    document.blocks.push(wall);
    let mut heading_block = heading("generic-heading", 2, "概述");
    heading_block.lint_ignore = vec!["QR418".into()];
    document.blocks.push(heading_block);

    let report = lint_document(&document);
    assert!(!has_code(&report, "QR316"));
    assert!(!has_code(&report, "QR416"));
    assert!(!has_code(&report, "QR417"));
    assert!(!has_code(&report, "QR418"));
    assert!(report.summary.suppressed >= 4, "{:?}", report.summary);

    document.lint.ignore = vec![
        "QR316".into(),
        "QR416".into(),
        "QR417".into(),
        "QR418".into(),
    ];
    let report = lint_document(&document);
    assert!(!has_code(&report, "QR316"));
    assert!(!has_code(&report, "QR416"));
    assert!(!has_code(&report, "QR417"));
    assert!(!has_code(&report, "QR418"));
}

#[test]
fn qr201_water_filling_does_not_flag_tables_that_fit_total_width() {
    let mut document = doc();
    let mut columns = vec![ColumnNode::leaf("metric", "指标名称")];
    for index in 0..5 {
        columns.push(ColumnNode::leaf(format!("q{index}"), format!("Q{index}")));
    }
    let rows: Vec<_> = (0..2)
        .map(|row_index| {
            let mut values = BTreeMap::new();
            values.insert(
                "metric".to_string(),
                CellValue::text(format!("指标 {}", row_index + 1)),
            );
            for column_index in 0..5 {
                values.insert(format!("q{column_index}"), CellValue::Integer(column_index));
            }
            values
        })
        .collect();
    document
        .blocks
        .push(table("water-filling", None, columns, rows));

    let report = lint_document(&document);
    assert!(
        !has_code(&report, "QR201"),
        "six columns whose total minimum width fits A4 must not trigger QR201: {:?}",
        report.diagnostics
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
fn style_and_config_rules_are_detected() {
    let mut css = doc();
    css.theme.custom_css = ".a { color: red;".into();
    let report = lint_document(&css);
    assert!(!finding(&report, "QR501").suppressible);

    let mut external = doc();
    external.blocks.push(custom_html(
        "<img src=\"https://example.com/a.png\">",
        "Image",
    ));
    assert!(has_code(&lint_document(&external), "QR502"));

    let mut overflow = doc();
    overflow.blocks.push(custom_html(
        "<div style=\"width: 960px\">wide</div>",
        "Wide card",
    ));
    assert!(has_code(&lint_document(&overflow), "QR503"));

    let mut duplicate_title = doc();
    duplicate_title
        .blocks
        .push(custom_html("<h2>KPI Grid</h2><div>18%</div>", "18%"));
    assert!(has_code(&lint_document(&duplicate_title), "QR504"));

    let mut fallback = doc();
    fallback.blocks.push(custom_html("<div>x</div>", ""));
    assert!(has_code(&lint_document(&fallback), "QR505"));

    let mut empty_html = doc();
    empty_html.blocks.push(custom_html("", "Fallback"));
    assert!(has_code(&lint_document(&empty_html), "QR506"));

    let mut token_name = doc();
    token_name
        .theme
        .tokens
        .insert("color".into(), "#fff".into());
    assert!(has_code(&lint_document(&token_name), "QR507"));

    let mut token_value = doc();
    token_value
        .theme
        .tokens
        .insert("--x".into(), "#fff; body{}".into());
    assert!(has_code(&lint_document(&token_value), "QR508"));

    let mut preset = doc();
    preset.theme.preset = String::new();
    assert!(has_code(&lint_document(&preset), "QR509"));

    let mut toc = doc();
    toc.toc.max_depth = 5;
    assert!(has_code(&lint_document(&toc), "QR510"));

    let mut toc_title = doc();
    toc_title.toc.title = String::new();
    assert!(has_code(&lint_document(&toc_title), "QR511"));

    let mut cover_title = doc();
    cover_title.cover.enabled = true;
    cover_title.cover.title = String::new();
    assert!(has_code(&lint_document(&cover_title), "QR512"));

    let mut cover_variant = doc();
    cover_variant.cover.enabled = true;
    cover_variant.cover.title = "Report".into();
    cover_variant.cover.variant = String::new();
    assert!(has_code(&lint_document(&cover_variant), "QR513"));
}

#[test]
fn layered_ignore_filters_suppress_only_allowlisted_rules() {
    let mut document = doc();
    document
        .blocks
        .push(paragraph("placeholder", "Pending TBD content."));
    document.blocks.push(table(
        "empty",
        None,
        vec![ColumnNode::leaf("a", "A"), ColumnNode::leaf("b", "B")],
        vec![
            row(&[("a", CellValue::text("x")), ("b", CellValue::Null)]),
            row(&[("a", CellValue::text("y")), ("b", CellValue::Null)]),
        ],
    ));

    let baseline = lint_document(&document);
    assert!(has_code(&baseline, "QR202"));
    assert!(has_code(&baseline, "QR401"));

    let mut doc_ignored = document.clone();
    doc_ignored.lint = LintConfig {
        ignore: vec!["QR202".into()],
    };
    let report = lint_document(&doc_ignored);
    assert!(!has_code(&report, "QR202"));
    assert_eq!(report.summary.suppressed, 1);

    let mut block_ignored = document.clone();
    block_ignored.blocks[1].lint_ignore = vec!["QR202".into()];
    let report = lint_document(&block_ignored);
    assert!(!has_code(&report, "QR202"));
    assert_eq!(report.summary.suppressed, 1);

    let report = lint_document_with_options(
        &document,
        &LintOptions {
            select: Vec::new(),
            ignore: vec!["QR2".into(), "QR401".into()],
        },
    );
    assert!(!has_code(&report, "QR202"));
    assert!(!has_code(&report, "QR401"));
    assert_eq!(report.summary.suppressed, 2);
}

#[test]
fn non_suppressible_rules_survive_every_ignore_layer() {
    let mut document = doc();
    document
        .blocks
        .push(paragraph("bad", "Revenue [^missing]."));
    document.lint = LintConfig {
        ignore: vec!["QR101".into()],
    };
    document.blocks[0].lint_ignore = vec!["QR101".into()];
    let report = lint_document_with_options(
        &document,
        &LintOptions {
            select: Vec::new(),
            ignore: vec!["QR101".into(), "QR1".into()],
        },
    );
    let diagnostic = finding(&report, "QR101");
    assert!(!diagnostic.suppressible);
    assert_eq!(report.summary.suppressed, 0);
}

#[test]
fn select_supports_codes_names_and_category_prefixes() {
    let mut document = doc();
    document.blocks.push(table(
        "empty",
        None,
        vec![ColumnNode::leaf("a", "A"), ColumnNode::leaf("b", "B")],
        vec![
            row(&[("a", CellValue::text("x")), ("b", CellValue::Null)]),
            row(&[("a", CellValue::text("y")), ("b", CellValue::Null)]),
        ],
    ));
    document.blocks.push(paragraph("placeholder", "TBD"));
    for token in ["QR2", "QR202", "table-empty-column"] {
        let report = lint_document_with_options(
            &document,
            &LintOptions {
                select: vec![token.into()],
                ..LintOptions::default()
            },
        );
        assert!(!report.diagnostics.is_empty(), "{token}");
        assert!(
            report
                .diagnostics
                .iter()
                .all(|diagnostic| diagnostic.code == "QR202"),
            "{token}: {:?}",
            report.diagnostics
        );
    }
    let report = lint_document_with_options(
        &document,
        &LintOptions {
            select: vec!["QR4".into()],
            ..LintOptions::default()
        },
    );
    assert!(
        report
            .diagnostics
            .iter()
            .all(|diagnostic| diagnostic.code == "QR401")
    );
}

#[test]
fn normalization_warnings_use_registry_codes() {
    let mut block = chart(serde_json::json!({
        "title": {"text": "Revenue", "top": "top"},
        "toolbox": {"show": true},
        "series": [{"type": "bar", "data": [1]}]
    }));
    let warnings = normalize_block(&mut block);
    assert!(warnings.iter().any(|warning| warning.code == "QR312"));
    assert!(warnings.iter().any(|warning| warning.code == "QR313"));
    assert!(warnings.iter().all(|warning| warning.suppressible));
}

#[test]
fn metadata_helpers_roundtrip_new_fields() {
    let mut document = valid_document();
    document.lint.ignore = vec!["QR202".into()];
    document.blocks[0].lint_ignore = vec!["QR409".into()];
    let json = serde_json::to_value(&document)
        .unwrap_or_else(|error| panic!("document serializes: {error}"));
    assert_eq!(json["lint"]["ignore"][0], "QR202");
    assert_eq!(json["blocks"][0]["lint_ignore"][0], "QR409");
    let parsed: ReportDocument = serde_json::from_value(json)
        .unwrap_or_else(|error| panic!("document deserializes: {error}"));
    assert_eq!(parsed, document);
}
