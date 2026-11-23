#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::indexing_slicing,
    clippy::panic
)]

pub mod common;
use common::*;

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
    if let BlockKind::Table(table) = &mut table_block.body {
        table.note = Some("口径见 [^jxufe_101: 公报]".into());
    }
    table_block.source_note = Some("来源 [jxufe_101]".into());
    document.blocks.push(table_block);

    let mut chart_block = chart_named(
        "note-chart",
        serde_json::json!({"series": [{"type": "bar", "data": [1]}]}),
    );
    if let BlockKind::Echarts(chart) = &mut chart_block.body {
        chart.note = Some("说明 [^jxufe_101, jxufe_102]".into());
    }
    document.blocks.push(chart_block);

    let mut html_block = custom_html("<div>KPI</div>", "KPI");
    if let BlockKind::CustomHtml(html) = &mut html_block.body {
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
