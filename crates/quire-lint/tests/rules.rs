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
    Block, BlockId, BlockKind, BlockStyle, CellValue, ColumnDataType, ColumnNode, CustomHtmlBlock,
    DocId, EchartsBlock, LintConfig, LintReport, ReportDocument, Severity, SourceEntry, SourceId,
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

fn with_table_note(mut block: Block, note: &str) -> Block {
    if let BlockKind::Table(ref mut table) = block.body {
        table.note = Some(note.into());
    }
    block
}

fn with_summary_row(mut block: Block, summary: BTreeMap<String, CellValue>) -> Block {
    if let BlockKind::Table(ref mut table) = block.body {
        table.summary_rows.push(summary);
    }
    block
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
    let mut data_table = table(
        "table",
        None,
        vec![
            ColumnNode::leaf("region", "Region"),
            ColumnNode::leaf("value", "Value（项）"),
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
    );
    data_table.source_note = Some("Annual filing".into());
    data_table.source_ids = vec![sid("sec10")];
    document.blocks.push(data_table);
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
fn qr103_requires_a_quantitative_unit_and_ignores_temporal_context() {
    for (id, text) in [
        ("year", "2025年，学校启动新一轮建设规划。"),
        ("duration", "近5年，学校持续完善内部治理。"),
        (
            "plan-range",
            "十四五（2021—2025年）期间，学校推进综合改革。",
        ),
        ("chapter", "第1章说明建设背景，第2章分析具体任务。"),
        ("aspects", "改革包括3个方面，以下分别说明。"),
    ] {
        let mut document = doc();
        document.blocks.push(paragraph(id, text));
        assert!(
            !has_code(&lint_document(&document), "QR103"),
            "temporal or structural numbers are not numeric claims: {text}"
        );
    }

    for (id, text) in [
        ("projects", "国家级项目立项214项。"),
        ("papers", "发表高水平论文32篇。"),
        ("universities", "本次对标覆盖12所高校。"),
        ("people", "引育国家级人才8名。"),
        ("rank", "综合实力位居第3位。"),
        ("multiple", "年度经费增长1.5倍。"),
        ("permille", "相关学科进入ESI全球前1‰。"),
        ("percentage-points", "占比提高2个百分点。"),
        ("degree-point", "新增3个博士点。"),
    ] {
        let mut document = doc();
        document.blocks.push(paragraph(id, text));
        assert!(
            has_code(&lint_document(&document), "QR103"),
            "higher-education quantitative units must be detected: {text}"
        );
    }
}

#[test]
fn qr103_accepts_a_real_block_source_note_without_forcing_source_id_padding() {
    let mut document = doc();
    let mut attributed = paragraph("projects", "国家级项目立项214项。");
    attributed.source_note = Some("国家自然科学基金委员会年度立项公告。".into());
    document.blocks.push(attributed);
    let report = lint_document(&document);
    assert!(!has_code(&report, "QR103"), "{:?}", report.diagnostics);

    document
        .blocks
        .push(paragraph("unattributed", "同期省级项目立项32项。"));
    assert!(has_code(&lint_document(&document), "QR103"));
}

#[test]
fn qr108_flags_low_authority_source_domains() {
    let mut document = doc();
    document.sources.insert(
        sid("baike"),
        SourceEntry {
            url: Some("https://www.Baike.Baidu.com:443/item/高校".into()),
            ..SourceEntry::new("百科")
        },
    );
    document.blocks.push(paragraph("cite", "资料 [^baike]。"));

    let report = lint_document(&document);
    let diagnostic = finding(&report, "QR108");
    assert_eq!(diagnostic.severity, Severity::Warning);
    assert!(diagnostic.suppressible);
    assert!(diagnostic.block_id.is_none());
    assert_eq!(diagnostic.path.as_deref(), Some("sources.baike.url"));
    assert!(diagnostic.message.contains("baike.baidu.com"));
    assert!(diagnostic.hint.contains(".gov.cn"));

    let mut subdomain = doc();
    subdomain.sources.insert(
        sid("sohu"),
        SourceEntry {
            url: Some("https://news.sohu.com/a".into()),
            ..SourceEntry::new("搜狐")
        },
    );
    subdomain.blocks.push(paragraph("cite", "资料 [^sohu]。"));
    assert!(has_code(&lint_document(&subdomain), "QR108"));

    let mut safe = doc();
    safe.sources.insert(
        sid("gov"),
        SourceEntry {
            url: Some("https://www.gov.cn/zhengce".into()),
            ..SourceEntry::new("国务院")
        },
    );
    safe.sources.insert(
        sid("evil"),
        SourceEntry {
            url: Some("https://baike.baidu.com.evil.com/item".into()),
            ..SourceEntry::new("仿冒")
        },
    );
    safe.blocks
        .push(paragraph("cite", "资料 [^gov] 与 [^evil]。"));
    assert!(!has_code(&lint_document(&safe), "QR108"));

    document.lint.ignore = vec!["QR108".into()];
    let report = lint_document(&document);
    assert!(!has_code(&report, "QR108"));
    assert!(report.summary.suppressed >= 1);
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
            row(&[
                ("a", CellValue::text("x")),
                ("b", CellValue::text("博士点")),
            ]),
            row(&[
                ("a", CellValue::text("y")),
                ("b", CellValue::text("博士点")),
            ]),
            row(&[
                ("a", CellValue::text("z")),
                ("b", CellValue::text("博士点")),
            ]),
        ],
    ));
    let report = lint_document(&constant);
    assert!(has_code(&report, "QR203"));
    assert!(finding(&report, "QR203").message.contains("博士点"));

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
fn qr205_allows_explicit_percentage_rows_in_indicator_tables() {
    let mut document = doc();
    document.blocks.push(table(
        "row-oriented-units",
        None,
        vec![
            ColumnNode::leaf("metric", "指标"),
            ColumnNode::leaf("ours", "我校"),
            ColumnNode::leaf("peer", "对标校"),
        ],
        vec![
            row(&[
                ("metric", CellValue::text("立项总数")),
                ("ours", CellValue::Integer(227)),
                ("peer", CellValue::Integer(245)),
            ]),
            row(&[
                ("metric", CellValue::text("面上项目")),
                ("ours", CellValue::Integer(93)),
                ("peer", CellValue::Integer(136)),
            ]),
            row(&[
                ("metric", CellValue::text("青年项目占比")),
                ("ours", CellValue::text("54.6%")),
                ("peer", CellValue::text("38.0%")),
            ]),
            row(&[
                ("metric", CellValue::text("面上项目占比")),
                ("ours", CellValue::text("41.0%")),
                ("peer", CellValue::text("55.5%")),
            ]),
        ],
    ));
    assert!(
        !has_code(&lint_document(&document), "QR205"),
        "explicit percentage indicator rows must not make entity columns look unit-inconsistent"
    );
}

#[test]
fn qr206_distinguishes_metric_time_totals_from_summary_rows() {
    let mut time_window = doc();
    time_window.blocks.push(table(
        "metric-time-total",
        None,
        vec![
            ColumnNode::leaf("metric", "指标"),
            ColumnNode::leaf("ours", "我校"),
            ColumnNode::leaf("peer", "对标校"),
        ],
        vec![
            row(&[
                ("metric", CellValue::text("国家自然科学基金（2025年）")),
                ("ours", CellValue::Integer(93)),
                ("peer", CellValue::Integer(81)),
            ]),
            row(&[
                ("metric", CellValue::text("国家自然科学基金（2026年）")),
                ("ours", CellValue::Integer(100)),
                ("peer", CellValue::Integer(98)),
            ]),
            row(&[
                (
                    "metric",
                    CellValue::text("国家科学技术奖（2023、2025年合计）"),
                ),
                ("ours", CellValue::Integer(1)),
                ("peer", CellValue::Integer(2)),
            ]),
        ],
    ));
    assert!(
        !has_code(&lint_document(&time_window), "QR206"),
        "a metric's own time-window total is not a whole-table summary row"
    );

    let mut entity_total = doc();
    entity_total.blocks.push(table(
        "entity-total",
        None,
        vec![
            ColumnNode::leaf("entity", "学校"),
            ColumnNode::leaf("count", "项目数").with_type(ColumnDataType::Number),
        ],
        vec![
            row(&[
                ("entity", CellValue::text("甲校")),
                ("count", CellValue::Integer(1)),
            ]),
            row(&[
                ("entity", CellValue::text("乙校")),
                ("count", CellValue::Integer(2)),
            ]),
            row(&[
                ("entity", CellValue::text("两校合计")),
                ("count", CellValue::Integer(3)),
            ]),
        ],
    ));
    assert!(!has_code(&lint_document(&entity_total), "QR206"));
}

#[test]
fn qr201_rejects_explicit_widths_below_readable_floors() {
    let mut document = doc();
    document.blocks.push(
        table(
            "narrow-entity",
            None,
            vec![
                ColumnNode::leaf("school", "高校"),
                ColumnNode::leaf("grade", "A+"),
            ],
            vec![row(&[
                ("school", CellValue::text("深圳大学")),
                ("grade", CellValue::text("A+")),
            ])],
        )
        .with_style(BlockStyle {
            column_widths: Some(BTreeMap::from([
                ("school".into(), "37px".into()),
                ("grade".into(), "37px".into()),
            ])),
            ..BlockStyle::default()
        }),
    );
    let report = lint_document(&document);
    let paths: Vec<_> = report
        .diagnostics
        .iter()
        .filter(|diagnostic| diagnostic.code == "QR201")
        .filter_map(|diagnostic| diagnostic.path.clone())
        .collect();
    assert!(paths.contains(&"blocks[0].columns.school".to_string()));
    assert!(paths.contains(&"blocks[0].columns.grade".to_string()));
    assert!(finding(&report, "QR201").hint.contains("最多不超过 9 列"));

    let mut readable = doc();
    readable.blocks.push(
        table(
            "readable-entity",
            None,
            vec![
                ColumnNode::leaf("school", "高校"),
                ColumnNode::leaf("grade", "A+"),
            ],
            vec![row(&[
                ("school", CellValue::text("深圳大学")),
                ("grade", CellValue::text("A+")),
            ])],
        )
        .with_style(BlockStyle {
            column_widths: Some(BTreeMap::from([
                ("school".into(), "76px".into()),
                ("grade".into(), "40px".into()),
            ])),
            ..BlockStyle::default()
        }),
    );
    assert!(!has_code(&lint_document(&readable), "QR201"));
}

#[test]
fn qr204_blocks_more_than_nine_columns_even_with_short_cells() {
    for (count, expected) in [(9, false), (10, true)] {
        let mut document = doc();
        let columns: Vec<_> = (0..count)
            .map(|index| ColumnNode::leaf(format!("c{index}"), format!("列{index}")))
            .collect();
        let values: BTreeMap<String, CellValue> = (0..count)
            .map(|index| (format!("c{index}"), CellValue::text("1")))
            .collect();
        document
            .blocks
            .push(table("column-count", None, columns, vec![values]));
        assert_eq!(has_code(&lint_document(&document), "QR204"), expected);
    }
}

#[test]
fn qr202_flags_columns_that_are_only_emdashes() {
    let mut document = doc();
    document.blocks.push(table(
        "empty-emdash",
        None,
        vec![
            ColumnNode::leaf("school", "高校"),
            ColumnNode::leaf("missing", "暂未收录指标"),
        ],
        vec![
            row(&[
                ("school", CellValue::text("深圳大学")),
                ("missing", CellValue::text("—")),
            ]),
            row(&[
                ("school", CellValue::text("广州大学")),
                ("missing", CellValue::text("—")),
            ]),
        ],
    ));
    let report = lint_document(&document);
    let diagnostic = finding(&report, "QR202");
    assert_eq!(diagnostic.path.as_deref(), Some("blocks[0].rows.missing"));
}

#[test]
fn qr206_validates_single_integer_summary_rows() {
    fn columns() -> Vec<ColumnNode> {
        vec![
            ColumnNode::leaf("name", "名称"),
            ColumnNode::leaf("count", "数量"),
        ]
    }

    fn rows(total: i64) -> Vec<BTreeMap<String, CellValue>> {
        vec![
            row(&[
                ("name", CellValue::text("甲")),
                ("count", CellValue::text("1,200")),
            ]),
            row(&[
                ("name", CellValue::text("乙")),
                ("count", CellValue::Integer(300)),
            ]),
            row(&[
                ("name", CellValue::text("合计")),
                ("count", CellValue::Integer(total)),
            ]),
        ]
    }

    let mut correct = doc();
    correct
        .blocks
        .push(table("correct", None, columns(), rows(1_500)));
    assert!(!has_code(&lint_document(&correct), "QR206"));

    let mut wrong = doc();
    wrong
        .blocks
        .push(table("wrong", None, columns(), rows(1_501)));
    let report = lint_document(&wrong);
    let diagnostic = finding(&report, "QR206");
    assert_eq!(diagnostic.severity, Severity::Warning);
    assert!(diagnostic.suppressible);
    assert_eq!(diagnostic.path.as_deref(), Some("blocks[0].rows[2].count"));
    assert!(diagnostic.message.contains("is `1501`"));
    assert!(diagnostic.message.contains("sum to `1500`"));
    assert!(diagnostic.hint.contains("to `1500`"));

    let mut placeholder = doc();
    placeholder.blocks.push(table(
        "placeholder",
        None,
        columns(),
        vec![
            row(&[
                ("name", CellValue::text("甲")),
                ("count", CellValue::text("—")),
            ]),
            row(&[
                ("name", CellValue::text("乙")),
                ("count", CellValue::Integer(5)),
            ]),
            row(&[
                ("name", CellValue::text("合计")),
                ("count", CellValue::Integer(5)),
            ]),
        ],
    ));
    assert!(!has_code(&lint_document(&placeholder), "QR206"));

    let mut rounded = doc();
    rounded.blocks.push(with_table_note(
        table("rounded", None, columns(), rows(1_501)),
        "四舍五入保留整数",
    ));
    assert!(!has_code(&lint_document(&rounded), "QR206"));

    let mut average = doc();
    let mut average_rows = rows(1_501);
    if let Some(last) = average_rows.last_mut() {
        last.insert("name".into(), CellValue::text("平均值"));
    }
    average
        .blocks
        .push(table("average", None, columns(), average_rows));
    assert!(!has_code(&lint_document(&average), "QR206"));

    let mut ignored = table("ignored", None, columns(), rows(1_501));
    ignored.lint_ignore = vec!["QR206".into()];
    let mut ignored_document = doc();
    ignored_document.blocks.push(ignored);
    let report = lint_document(&ignored_document);
    assert!(!has_code(&report, "QR206"));
    assert!(report.summary.suppressed >= 1);
}

#[test]
fn qr206_validates_composite_integer_summary_rows() {
    fn columns() -> Vec<ColumnNode> {
        vec![
            ColumnNode::leaf("name", "名称"),
            ColumnNode::leaf("counts", "项目/经费"),
        ]
    }

    let mut slash = doc();
    slash.blocks.push(table(
        "slash",
        None,
        columns(),
        vec![
            row(&[
                ("name", CellValue::text("甲")),
                ("counts", CellValue::text("3531 / 934")),
            ]),
            row(&[
                ("name", CellValue::text("乙")),
                ("counts", CellValue::text("3000 / 900")),
            ]),
            row(&[
                ("name", CellValue::text("总计")),
                ("counts", CellValue::text("6532 / 1835")),
            ]),
        ],
    ));
    let report = lint_document(&slash);
    let diagnostic = finding(&report, "QR206");
    assert!(diagnostic.message.contains("is `6532 / 1835`"));
    assert!(diagnostic.message.contains("sum to `6531 / 1834`"));
    assert!(diagnostic.hint.contains("to `6531 / 1834`"));

    let mut slash_correct = doc();
    slash_correct.blocks.push(table(
        "slash-correct",
        None,
        columns(),
        vec![
            row(&[
                ("name", CellValue::text("甲")),
                ("counts", CellValue::text("3531 / 934")),
            ]),
            row(&[
                ("name", CellValue::text("乙")),
                ("counts", CellValue::text("3000 / 900")),
            ]),
            row(&[
                ("name", CellValue::text("总计")),
                ("counts", CellValue::text("6531 / 1834")),
            ]),
        ],
    ));
    assert!(!has_code(&lint_document(&slash_correct), "QR206"));

    let mut middle_dot = doc();
    middle_dot.blocks.push(table(
        "middle-dot",
        None,
        columns(),
        vec![
            row(&[
                ("name", CellValue::text("甲")),
                ("counts", CellValue::text("8·27·35")),
            ]),
            row(&[
                ("name", CellValue::text("乙")),
                ("counts", CellValue::text("2·3·5")),
            ]),
            row(&[
                ("name", CellValue::text("合计")),
                ("counts", CellValue::text("10·31·40")),
            ]),
        ],
    ));
    let report = lint_document(&middle_dot);
    let diagnostic = finding(&report, "QR206");
    assert!(diagnostic.message.contains("is `10·31·40`"));
    assert!(diagnostic.message.contains("sum to `10·30·40`"));
    assert!(diagnostic.hint.contains("to `10·30·40`"));
}

#[test]
fn qr206_validates_full_width_composite_separator() {
    let mut document = doc();
    document.blocks.push(table(
        "full-width",
        None,
        vec![
            ColumnNode::leaf("name", "名称"),
            ColumnNode::leaf("counts", "项目／经费"),
        ],
        vec![
            row(&[
                ("name", CellValue::text("甲")),
                ("counts", CellValue::text("5／7")),
            ]),
            row(&[
                ("name", CellValue::text("乙")),
                ("counts", CellValue::text("1／2")),
            ]),
            row(&[
                ("name", CellValue::text("合计")),
                ("counts", CellValue::text("7／9")),
            ]),
        ],
    ));
    let report = lint_document(&document);
    let diagnostic = finding(&report, "QR206");
    assert!(diagnostic.message.contains("is `7／9`"));
    assert!(diagnostic.message.contains("sum to `6／9`"));
}

#[test]
fn qr206_validates_decimal_values_in_summary_rows() {
    fn decimal_table(total: &str) -> Block {
        with_summary_row(
            table(
                "decimal-summary",
                None,
                vec![
                    ColumnNode::leaf("name", "名称"),
                    ColumnNode::leaf("funding", "科研经费（万元）")
                        .with_type(ColumnDataType::Currency),
                ],
                vec![
                    row(&[
                        ("name", CellValue::text("甲")),
                        ("funding", CellValue::text("1,245.60")),
                    ]),
                    row(&[
                        ("name", CellValue::text("乙")),
                        ("funding", CellValue::Float(300.25)),
                    ]),
                ],
            ),
            row(&[
                ("name", CellValue::text("合计")),
                ("funding", CellValue::text(total)),
            ]),
        )
    }

    let mut correct = doc();
    correct.blocks.push(decimal_table("1545.85"));
    assert!(!has_code(&lint_document(&correct), "QR206"));

    let mut wrong = doc();
    wrong.blocks.push(decimal_table("1545.86"));
    let report = lint_document(&wrong);
    let diagnostic = finding(&report, "QR206");
    assert_eq!(
        diagnostic.path.as_deref(),
        Some("blocks[0].summary_rows[0].funding")
    );
    assert!(diagnostic.message.contains("is `1545.86`"));
    assert!(diagnostic.message.contains("sum to `1545.85`"));
    assert!(diagnostic.hint.contains("1545.85"));
}

#[test]
fn qr206_skips_non_additive_columns_and_applies_decimal_rounding_tolerance() {
    fn rounded_table(total: &str) -> Block {
        with_summary_row(
            table(
                "rounded-summary",
                None,
                vec![
                    ColumnNode::leaf("name", "名称"),
                    ColumnNode::leaf("rank", "排名").with_type(ColumnDataType::Number),
                    ColumnNode::leaf("share", "占比").with_type(ColumnDataType::Percent),
                    ColumnNode::leaf("amount", "金额").with_type(ColumnDataType::Number),
                ],
                vec![
                    row(&[
                        ("name", CellValue::text("甲")),
                        ("rank", CellValue::Integer(1)),
                        ("share", CellValue::text("10%")),
                        ("amount", CellValue::text("0.52")),
                    ]),
                    row(&[
                        ("name", CellValue::text("乙")),
                        ("rank", CellValue::Integer(2)),
                        ("share", CellValue::text("20%")),
                        ("amount", CellValue::text("0.52")),
                    ]),
                ],
            ),
            row(&[
                ("name", CellValue::text("合计")),
                ("rank", CellValue::Integer(99)),
                ("share", CellValue::text("30%")),
                ("amount", CellValue::text(total)),
            ]),
        )
    }

    let mut rounded = doc();
    rounded.blocks.push(rounded_table("1.0"));
    assert!(
        !has_code(&lint_document(&rounded), "QR206"),
        "rank/rate columns must be skipped and summary precision supplies the rounding tolerance"
    );

    let mut wrong = doc();
    wrong.blocks.push(rounded_table("1.2"));
    let report = lint_document(&wrong);
    let hits: Vec<_> = report
        .diagnostics
        .iter()
        .filter(|diagnostic| diagnostic.code == "QR206")
        .collect();
    assert_eq!(hits.len(), 1, "{:?}", report.diagnostics);
    assert_eq!(
        hits[0].path.as_deref(),
        Some("blocks[0].summary_rows[0].amount")
    );
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
            note: Some("单位：项；统计口径：2025年。".into()),
        },
    );
    assert!(
        isolated
            .diagnostics
            .iter()
            .all(|diagnostic| diagnostic.code == "QR422"),
        "{:?}",
        isolated.diagnostics
    );
    assert!(has_code(&isolated, "QR422"));
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
fn qr317_flags_unguarded_line_end_labels() {
    let mut unguarded = doc();
    unguarded.blocks.push(chart_named(
        "line-end",
        serde_json::json!({
            "series": [{
                "type": "line",
                "data": [1, 2],
                "endLabel": {"show": true}
            }]
        }),
    ));
    let report = lint_document(&unguarded);
    let diagnostic = finding(&report, "QR317");
    assert_eq!(diagnostic.severity, Severity::Warning);
    assert!(diagnostic.suppressible);
    assert!(diagnostic.message.contains("endLabel"));
    assert_eq!(diagnostic.path.as_deref(), Some("blocks[0].option.series"));

    let mut fixed = doc();
    fixed.blocks.push(chart_named(
        "line-end-fixed",
        serde_json::json!({
            "series": [{
                "type": "line",
                "data": [1, 2],
                "endLabel": {"show": true, "textBorderWidth": 0}
            }]
        }),
    ));
    assert!(!has_code(&lint_document(&fixed), "QR317"));

    let mut multiple = doc();
    multiple.blocks.push(chart_named(
        "line-end-multiple",
        serde_json::json!({
            "series": [
                {"type": "line", "data": [1, 2], "endLabel": {"show": true, "textBorderWidth": 0}},
                {"type": "line", "data": [2, 3], "endLabel": {"show": true, "textBorderWidth": 0}}
            ]
        }),
    ));
    assert!(has_code(&lint_document(&multiple), "QR317"));

    let mut guarded = doc();
    guarded.blocks.push(chart_named(
        "line-end-guarded",
        serde_json::json!({
            "labelLayout": {"hideOverlap": true},
            "series": [
                {"type": "line", "data": [1, 2], "endLabel": {"show": true, "textBorderWidth": 0}},
                {"type": "line", "data": [2, 3], "endLabel": {"show": true, "textBorderWidth": 0}}
            ]
        }),
    ));
    assert!(!has_code(&lint_document(&guarded), "QR317"));

    let mut ignored = chart_named(
        "line-end-ignored",
        serde_json::json!({
            "series": [{
                "type": "line",
                "data": [1, 2],
                "endLabel": {"show": true}
            }]
        }),
    );
    ignored.lint_ignore = vec!["QR317".into()];
    let mut ignored_doc = doc();
    ignored_doc.blocks.push(ignored);
    let report = lint_document(&ignored_doc);
    assert!(!has_code(&report, "QR317"));
    assert!(report.summary.suppressed >= 1);
}

#[test]
fn qr318_flags_small_integer_axes_without_min_interval() {
    let mut document = doc();
    document.blocks.push(chart_named(
        "small-int",
        serde_json::json!({
            "xAxis": {"type": "category", "data": ["A", "B", "C"]},
            "yAxis": {"type": "value"},
            "series": [{"type": "bar", "data": [0, 1, 2]}]
        }),
    ));
    let report = lint_document(&document);
    let diagnostic = finding(&report, "QR318");
    assert_eq!(diagnostic.severity, Severity::Warning);
    assert!(diagnostic.suppressible);
    assert!(diagnostic.message.contains("max 2"));
    assert!(diagnostic.message.contains("yAxis"));
    assert_eq!(diagnostic.path.as_deref(), Some("blocks[0].option.yAxis"));
    assert!(diagnostic.hint.contains("minInterval"));

    let mut line = doc();
    line.blocks.push(chart_named(
        "small-line",
        serde_json::json!({
            "xAxis": {"type": "category", "data": ["A", "B"]},
            "yAxis": {"type": "value"},
            "series": [{"type": "line", "data": [1, 2]}]
        }),
    ));
    assert!(has_code(&lint_document(&line), "QR318"));

    for (id, y_axis) in [
        (
            "min-interval-one",
            serde_json::json!({"type": "value", "minInterval": 1}),
        ),
        (
            "interval-one",
            serde_json::json!({"type": "value", "interval": 1}),
        ),
        (
            "min-interval-two",
            serde_json::json!({"type": "value", "minInterval": 2}),
        ),
    ] {
        let mut fixed = doc();
        fixed.blocks.push(chart_named(
            id,
            serde_json::json!({
                "xAxis": {"type": "category", "data": ["A", "B", "C"]},
                "yAxis": y_axis,
                "series": [{"type": "bar", "data": [0, 1, 2]}]
            }),
        ));
        assert!(!has_code(&lint_document(&fixed), "QR318"), "{id}");
    }

    for (id, data) in [
        ("decimal", serde_json::json!([0.5, 1])),
        ("large", serde_json::json!([1, 16])),
    ] {
        let mut skipped = doc();
        skipped.blocks.push(chart_named(
            id,
            serde_json::json!({
                "xAxis": {"type": "category", "data": ["A", "B"]},
                "yAxis": {"type": "value"},
                "series": [{"type": "bar", "data": data}]
            }),
        ));
        assert!(!has_code(&lint_document(&skipped), "QR318"), "{id}");
    }

    let mut dual_axis = doc();
    dual_axis.blocks.push(chart_named(
        "dual-axis",
        serde_json::json!({
            "xAxis": {"type": "category", "data": ["A", "B"]},
            "yAxis": [{"type": "value"}, {"type": "value", "name": "增长率（%）"}],
            "series": [
                {"type": "bar", "yAxisIndex": 0, "data": [1, 2]},
                {"type": "line", "yAxisIndex": 1, "data": [1, 2]}
            ]
        }),
    ));
    let dual_report = lint_document(&dual_axis);
    let dual_hits: Vec<_> = dual_report
        .diagnostics
        .iter()
        .filter(|diagnostic| diagnostic.code == "QR318")
        .collect();
    assert_eq!(dual_hits.len(), 1, "{:?}", dual_report.diagnostics);
    assert_eq!(
        dual_hits[0].path.as_deref(),
        Some("blocks[0].option.yAxis[0]")
    );

    let mut multi_grid = doc();
    multi_grid.blocks.push(chart_named(
        "multi-grid",
        serde_json::json!({
            "xAxis": [
                {"type": "category", "data": ["A", "B"]},
                {"type": "category", "data": ["C", "D"]}
            ],
            "yAxis": [{"type": "value"}, {"type": "value"}],
            "series": [
                {"type": "bar", "xAxisIndex": 0, "yAxisIndex": 0, "data": [1, 2]},
                {"type": "bar", "xAxisIndex": 1, "yAxisIndex": 1, "data": [3, 4]}
            ]
        }),
    ));
    let multi_report = lint_document(&multi_grid);
    let multi_paths: Vec<_> = multi_report
        .diagnostics
        .iter()
        .filter(|diagnostic| diagnostic.code == "QR318")
        .filter_map(|diagnostic| diagnostic.path.as_deref())
        .collect();
    assert_eq!(multi_paths.len(), 2, "{:?}", multi_report.diagnostics);
    assert!(multi_paths.contains(&"blocks[0].option.yAxis[0]"));
    assert!(multi_paths.contains(&"blocks[0].option.yAxis[1]"));

    let mut dataset = doc();
    dataset.blocks.push(chart_named(
        "dataset",
        serde_json::json!({
            "xAxis": {"type": "category"},
            "yAxis": {"type": "value"},
            "dataset": {"source": [["A", 1], ["B", 2]]},
            "series": [{"type": "bar"}]
        }),
    ));
    assert!(
        !has_code(&lint_document(&dataset), "QR318"),
        "dataset/encode dimensions are intentionally skipped until they can be resolved safely"
    );

    let mut pie = doc();
    pie.blocks.push(chart_named(
        "pie",
        serde_json::json!({
            "xAxis": {"type": "category", "data": ["A", "B"]},
            "yAxis": {"type": "value"},
            "series": [{"type": "pie", "data": [1, 2]}]
        }),
    ));
    assert!(!has_code(&lint_document(&pie), "QR318"));

    let mut polar = doc();
    polar.blocks.push(chart_named(
        "polar",
        serde_json::json!({
            "polar": {},
            "xAxis": {"type": "category", "data": ["A", "B"]},
            "yAxis": {"type": "value"},
            "series": [{"type": "bar", "data": [1, 2]}]
        }),
    ));
    assert!(!has_code(&lint_document(&polar), "QR318"));

    let mut horizontal = doc();
    horizontal.blocks.push(chart_named(
        "horizontal",
        serde_json::json!({
            "xAxis": {"type": "value"},
            "yAxis": {"type": "category", "data": ["A", "B"]},
            "series": [{"type": "bar", "data": [1, 2]}]
        }),
    ));
    let report = lint_document(&horizontal);
    assert_eq!(
        finding(&report, "QR318").path.as_deref(),
        Some("blocks[0].option.xAxis")
    );

    let mut ignored = chart_named(
        "ignored",
        serde_json::json!({
            "xAxis": {"type": "category", "data": ["A", "B"]},
            "yAxis": {"type": "value"},
            "series": [{"type": "bar", "data": [1, 2]}]
        }),
    );
    ignored.lint_ignore = vec!["QR318".into()];
    let mut ignored_document = doc();
    ignored_document.blocks.push(ignored);
    let report = lint_document(&ignored_document);
    assert!(!has_code(&report, "QR318"));
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
    if let BlockKind::Echarts(ref mut chart_block) = chart.body {
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
    if let BlockKind::CustomHtml(ref mut custom) = html.body {
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
    if let BlockKind::Table(ref mut table) = table_block.body {
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
    if let BlockKind::Echarts(ref mut chart) = chart_block.body {
        chart.note = Some("长".repeat(181));
    }
    long_chart_note.blocks.push(chart_block);
    let report = lint_document(&long_chart_note);
    let diagnostic = finding(&report, "QR422");
    assert!(diagnostic.message.contains("当前 181 字"));

    let mut boundary_note = doc();
    let mut table_block = make_table("boundary-table-note");
    table_block.source_note = Some("教育部".into());
    if let BlockKind::Table(ref mut table) = table_block.body {
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
fn qr201_includes_summary_row_width_in_numeric_columns() {
    let mut narrow = doc();
    narrow.blocks.push(
        table(
            "narrow-summary",
            None,
            vec![
                ColumnNode::leaf("name", "名称"),
                ColumnNode::leaf("count", "数量").with_type(ColumnDataType::Number),
            ],
            vec![
                row(&[
                    ("name", CellValue::text("甲")),
                    ("count", CellValue::Integer(1)),
                ]),
                row(&[
                    ("name", CellValue::text("乙")),
                    ("count", CellValue::Integer(2)),
                ]),
                row(&[
                    ("name", CellValue::text("合计")),
                    ("count", CellValue::Integer(3_531)),
                ]),
            ],
        )
        .with_style(BlockStyle {
            column_widths: Some(BTreeMap::from([("count".into(), "48px".into())])),
            ..BlockStyle::default()
        }),
    );
    let report = lint_document(&narrow);
    let diagnostic = finding(&report, "QR201");
    assert_eq!(diagnostic.path.as_deref(), Some("blocks[0].columns.count"));
    assert!(diagnostic.message.contains("readable body"));

    let mut wide = doc();
    wide.blocks.push(
        table(
            "wide-summary",
            None,
            vec![
                ColumnNode::leaf("name", "名称"),
                ColumnNode::leaf("count", "数量").with_type(ColumnDataType::Number),
            ],
            vec![
                row(&[
                    ("name", CellValue::text("甲")),
                    ("count", CellValue::Integer(1)),
                ]),
                row(&[
                    ("name", CellValue::text("乙")),
                    ("count", CellValue::Integer(2)),
                ]),
                row(&[
                    ("name", CellValue::text("合计")),
                    ("count", CellValue::Integer(3_531)),
                ]),
            ],
        )
        .with_style(BlockStyle {
            column_widths: Some(BTreeMap::from([("count".into(), "56px".into())])),
            ..BlockStyle::default()
        }),
    );
    assert!(!has_code(&lint_document(&wide), "QR201"));
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
    let mut empty_column_table = table(
        "empty",
        None,
        vec![ColumnNode::leaf("a", "A"), ColumnNode::leaf("b", "B")],
        vec![
            row(&[("a", CellValue::text("x")), ("b", CellValue::Null)]),
            row(&[("a", CellValue::text("y")), ("b", CellValue::Null)]),
        ],
    );
    empty_column_table.source_note = Some("教育部".into());
    document.blocks.push(empty_column_table);
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

#[test]
fn qr205_exempts_row_oriented_indicator_tables_with_parenthesized_units() {
    let mut document = doc();
    let mut gap_table = table(
        "tab_gap",
        Some("关键指标现值与目标差距测算"),
        vec![
            ColumnNode::leaf("metric", "核心指标"),
            ColumnNode::leaf("current", "2025现值"),
            ColumnNode::leaf("target_2027", "2027目标"),
            ColumnNode::leaf("target_2030", "2030目标"),
        ],
        vec![
            row(&[
                ("metric", CellValue::text("现有高层次人才（人）")),
                ("current", CellValue::Integer(15)),
                ("target_2027", CellValue::Integer(25)),
                ("target_2030", CellValue::Integer(40)),
            ]),
            row(&[
                ("metric", CellValue::text("领军人才数量省占比（%）")),
                ("current", CellValue::text("0.34")),
                ("target_2027", CellValue::text("0.55")),
                ("target_2030", CellValue::text("0.80")),
            ]),
            row(&[
                ("metric", CellValue::text("国家自然科学基金年均立项（项）")),
                ("current", CellValue::text("24.7")),
                ("target_2027", CellValue::Integer(35)),
                ("target_2030", CellValue::Integer(45)),
            ]),
            row(&[
                ("metric", CellValue::text("教师研究领域产业契合度（分）")),
                ("current", CellValue::text("51.1")),
                ("target_2027", CellValue::text("60.0")),
                ("target_2030", CellValue::text("68.0")),
            ]),
        ],
    );
    gap_table.source_note = Some("省重点建设高校监测平台".into());
    document.blocks.push(gap_table);
    assert!(
        !has_code(&lint_document(&document), "QR205"),
        "row-oriented indicator table with per-row units must not trigger QR205"
    );
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
