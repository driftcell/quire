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
    ReportDocument::new(DocId::parse("lint-doc").unwrap())
}

fn bid(raw: &str) -> BlockId {
    BlockId::parse(raw).unwrap()
}

fn sid(raw: &str) -> SourceId {
    SourceId::parse(raw).unwrap()
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
    Block::new(
        bid("chart"),
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
    let json = serde_json::to_value(&document).unwrap();
    assert_eq!(json["lint"]["ignore"][0], "QR202");
    assert_eq!(json["blocks"][0]["lint_ignore"][0], "QR409");
    let parsed: ReportDocument = serde_json::from_value(json).unwrap();
    assert_eq!(parsed, document);
}
