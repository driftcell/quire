#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::indexing_slicing,
    clippy::panic
)]

pub mod common;
use common::*;

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
