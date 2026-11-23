#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::indexing_slicing,
    clippy::panic
)]

pub mod common;
use common::*;

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
fn qr201_and_qr204_accept_seven_column_benchmark_tables_with_two_line_headers() {
    let mut document = doc();
    document.blocks.push(table(
        "v1-overview",
        Some("核心竞争力基线"),
        vec![
            ColumnNode::leaf("school", "高校"),
            ColumnNode::leaf("chem", "化学工程与技术"),
            ColumnNode::leaf("safety", "安全科学与工程"),
            ColumnNode::leaf("lab", "全国重点实验室").with_type(ColumnDataType::Number),
            ColumnNode::leaf("talent", "高层次人才").with_type(ColumnDataType::Number),
            ColumnNode::leaf("nsfc", "国自然年均").with_type(ColumnDataType::Number),
            ColumnNode::leaf("award", "国家科技奖").with_type(ColumnDataType::Number),
        ],
        vec![
            row(&[
                ("school", CellValue::text("南京工业大学")),
                ("chem", CellValue::text("B+档（第14）")),
                ("safety", CellValue::text("B+档（第5）")),
                ("lab", CellValue::Integer(1)),
                ("talent", CellValue::Integer(165)),
                ("nsfc", CellValue::Float(157.8)),
                ("award", CellValue::Integer(2)),
            ]),
            row(&[
                ("school", CellValue::text("南京信息工程大学")),
                ("chem", CellValue::text("—")),
                ("safety", CellValue::text("—")),
                ("lab", CellValue::Integer(1)),
                ("talent", CellValue::Integer(188)),
                ("nsfc", CellValue::Float(234.4)),
                ("award", CellValue::Integer(0)),
            ]),
        ],
    ));
    let report = lint_document(&document);
    assert!(
        !has_code(&report, "QR201") && !has_code(&report, "QR204"),
        "7-column benchmark table with 2-line wrapped headers must fit A4 portrait without QR201/QR204: {:?}",
        report.diagnostics
    );
}
