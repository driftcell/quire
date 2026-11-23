use super::*;
use crate::test_support::*;
use crate::{
    block::{Block, CellValue, ColumnDataType, ColumnNode},
    model::{ReportDocument, SourceEntry},
};
use std::collections::BTreeMap;

fn sample() -> ReportDocument {
    let mut doc = ReportDocument::new(doc_id("md-report"));
    doc.revision = revision(4);
    doc.cover.title = "Q3 Review".into();
    doc.cover.subtitle = Some("Momentum".into());
    doc.cover.authors = vec!["Ada Lovelace".into()];
    doc.cover.date = Some("2026-10-01".into());
    doc.sources
        .insert(source_id("sec10"), SourceEntry::new("Annual filing 10-K"));
    doc.blocks
        .push(Block::heading(block_id("intro"), 1, "Introduction"));
    doc.blocks.push(
        Block::paragraph(
            block_id("summary"),
            "Revenue grew **18%** [^sec10] with `automation` momentum.",
        )
        .with_sources([source_id("sec10")]),
    );
    let mut row1 = BTreeMap::new();
    row1.insert("region".to_string(), CellValue::text("North America"));
    row1.insert("q1".to_string(), CellValue::Integer(100));
    row1.insert("q2".to_string(), CellValue::Integer(120));
    let mut row2 = BTreeMap::new();
    row2.insert("region".to_string(), CellValue::text("EMEA"));
    row2.insert("q1".to_string(), CellValue::Integer(80));
    row2.insert("q2".to_string(), CellValue::Integer(95));
    let mut summary = BTreeMap::new();
    summary.insert("region".to_string(), CellValue::text("Total"));
    summary.insert("q1".to_string(), CellValue::Integer(180));
    summary.insert("q2".to_string(), CellValue::Integer(215));
    doc.blocks.push(Block::table(
        block_id("financials"),
        TableBlock {
            title: Some("Revenue by region".into()),
            numbering: None,
            note: None,
            columns: vec![
                ColumnNode::leaf("region", "Region"),
                ColumnNode::group(
                    "revenue",
                    "Revenue",
                    vec![
                        ColumnNode::leaf("q1", "Q1").with_type(ColumnDataType::Currency),
                        ColumnNode::leaf("q2", "Q2").with_type(ColumnDataType::Currency),
                    ],
                ),
            ],
            rows: vec![row1, row2],
            summary_rows: vec![summary],
        },
    ));
    doc.blocks.push(Block::new(
        block_id("chart"),
        BlockKind::Echarts(EchartsBlock {
            title: Some("Revenue trend".into()),
            height: 320,
            option: serde_json::json!({
                "xAxis": {"data": ["Q1", "Q2"]},
                "series": [{"name": "North America", "type": "bar", "data": [100, 120]}]
            }),
            option_patch: None,
            fallback_summary: "Revenue rose from 100 to 120.".into(),
            numbering: None,
            note: None,
        }),
    ));
    doc.blocks
        .push(Block::heading(block_id("appendix"), 1, "Appendix"));
    doc.blocks
        .push(Block::paragraph(block_id("notes"), "Appendix notes"));
    doc
}

#[test]
fn full_projection_has_anchors_expanded_citations_and_flattened_tables() {
    let output = project_markdown(&sample(), &MarkdownOptions::default()).unwrap();
    assert!(output.contains("<!-- report:md-report | revision:4 -->"));
    assert!(output.contains("<!-- block:summary | type:paragraph -->"));
    assert!(output.contains("Revenue grew **18%** [^sec10] with `automation` momentum."));
    assert!(output.contains("Sources: [^sec10] (Annual filing 10-K)"));
    assert!(!output.contains("[^sec10:"));
    assert!(output.contains("**表 1 — Revenue by region**"));
    assert!(output.contains("| Region | Revenue / Q1 | Revenue / Q2 |"));
    assert!(output.contains("| :--- | ---: | ---: |"));
    assert!(output.contains("| **Total** | **180** | **215** |"));
    assert!(output.contains("<!-- block:chart | type:echarts -->"));
    assert!(output.contains("**图 1 — Revenue trend**"));
    assert!(output.contains("| Q1 | 100 |"));
}

#[test]
fn chart_data_can_be_omitted() {
    let options = MarkdownOptions {
        include_chart_data: false,
        ..Default::default()
    };
    let output = project_markdown(&sample(), &options).unwrap();
    assert!(output.contains("Revenue rose from 100 to 120."));
    assert!(!output.contains("_Chart data:"));
}

fn chart_doc(option: serde_json::Value) -> ReportDocument {
    let mut doc = ReportDocument::new(doc_id("chart-report"));
    doc.blocks.push(Block::new(
        block_id("chart"),
        BlockKind::Echarts(EchartsBlock {
            title: Some("Chart".into()),
            height: 320,
            option,
            option_patch: None,
            fallback_summary: "Fallback summary.".into(),
            numbering: None,
            note: None,
        }),
    ));
    doc
}

fn project_chart(option: serde_json::Value) -> String {
    project_markdown(&chart_doc(option), &MarkdownOptions::default()).unwrap()
}

#[test]
fn projects_dataset_arrays_and_object_rows() {
    let output = project_chart(serde_json::json!({
        "dataset": {"source": [
            ["Quarter", "Revenue", "Margin"],
            ["Q1", 120, 62.5],
            ["Q2", 150, 64.0]
        ]},
        "series": [{"type": "bar", "datasetIndex": 0}]
    }));
    assert!(output.contains("_Chart data: Dataset 1 (dataset)_"));
    assert!(output.contains("| Quarter | Revenue | Margin |"));
    assert!(output.contains("| Q1 | 120 | 62.5 |"));

    let output = project_chart(serde_json::json!({
        "dataset": [
            {"id": "kpi", "source": [
                {"region": "EMEA", "value": 10},
                {"region": "APAC", "value": 20}
            ]},
            {"id": "extra", "source": [
                ["Metric", "Value"],
                ["NPS", 42]
            ]}
        ],
        "series": [{"type": "bar", "datasetIndex": 0}]
    }));
    assert!(output.contains("_Chart data: kpi (dataset)_"));
    assert!(output.contains("| region | value |"));
    assert!(output.contains("| EMEA | 10 |"));
    assert!(output.contains("_Chart data: extra (dataset)_"));
    assert!(output.contains("| NPS | 42 |"));
}

#[test]
fn projects_multi_series_cartesian_table() {
    let output = project_chart(serde_json::json!({
        "xAxis": {"data": ["Q1", "Q2"]},
        "series": [
            {"name": "Revenue", "type": "bar", "data": [120, 150]},
            {"name": "Margin", "type": "line", "data": [62.5, 64.0]}
        ]
    }));
    assert!(output.contains("_Chart data: Revenue, Margin (bar, line)_"));
    assert!(output.contains("| Category | Revenue | Margin |"));
    assert!(output.contains("| Q1 | 120 | 62.5 |"));
    assert!(output.contains("| Q2 | 150 | 64 |"));
}

#[test]
fn projects_multi_panel_chart_and_multiple_chart_blocks() {
    let mut doc = chart_doc(serde_json::json!({
        "grid": [{}, {}],
        "xAxis": [
            {"gridIndex": 0, "data": ["Q1", "Q2"]},
            {"gridIndex": 1, "data": ["East", "West"]}
        ],
        "yAxis": [{"gridIndex": 0}, {"gridIndex": 1}],
        "title": [
            {"text": "Trend", "left": "4%", "top": 4, "textStyle": {"fontSize": 12}},
            {"text": "Structure", "left": "54%", "top": 4, "textStyle": {"fontSize": 12}}
        ],
        "series": [
            {"name": "Revenue", "type": "bar", "xAxisIndex": 0, "yAxisIndex": 0, "data": [10, 20]},
            {"name": "Share", "type": "line", "xAxisIndex": 1, "yAxisIndex": 1, "data": [30, 40]}
        ]
    }));
    doc.blocks.push(Block::new(
        block_id("chart2"),
        BlockKind::Echarts(EchartsBlock {
            title: Some("Mix".into()),
            height: 320,
            option: serde_json::json!({
                "series": [{"name": "Mix", "type": "pie", "data": [
                    {"name": "A", "value": 60},
                    {"name": "B", "value": 40}
                ]}]
            }),
            option_patch: None,
            fallback_summary: "The mix is 60/40.".into(),
            numbering: None,
            note: None,
        }),
    ));

    let output = project_markdown(&doc, &MarkdownOptions::default()).unwrap();
    assert!(output.contains("_Chart data: Revenue (bar) · panel 1_"));
    assert!(output.contains("| Q1 | 10 |"));
    assert!(output.contains("_Chart data: Share (line) · panel 2_"));
    assert!(output.contains("| East | 30 |"));
    assert!(output.contains("<!-- block:chart2 | type:echarts -->"));
    assert!(output.contains("_Chart data: Mix (pie)_"));
    assert!(output.contains("| A | 60 |"));
    assert!(output.contains("| B | 40 |"));
}

#[test]
fn projects_relation_charts_as_source_target_value() {
    let output = project_chart(serde_json::json!({
        "series": [
            {
                "name": "Flow",
                "type": "sankey",
                "data": [{"name": "A"}, {"name": "B"}],
                "links": [
                    {"source": "A", "target": "B", "value": 5},
                    {"source": "B", "target": "C", "value": 3}
                ]
            },
            {
                "name": "Network",
                "type": "graph",
                "data": [{"name": "X"}, {"name": "Y"}],
                "edges": [{"from": "X", "to": "Y", "value": 9}]
            }
        ]
    }));
    assert!(output.contains("_Chart data: Flow (sankey links)_"));
    assert!(output.contains("| Source | Target | Value |"));
    assert!(output.contains("| A | B | 5 |"));
    assert!(output.contains("| B | C | 3 |"));
    assert!(output.contains("_Chart data: Network (graph links)_"));
    assert!(output.contains("| X | Y | 9 |"));
}

#[test]
fn projects_radar_indicators_as_columns() {
    let output = project_chart(serde_json::json!({
        "radar": {"indicator": [{"name": "Revenue"}, {"name": "Margin"}]},
        "series": [{
            "name": "Plan",
            "type": "radar",
            "data": [
                {"name": "Actual", "value": [10, 20]},
                {"name": "Target", "value": [12, 18]}
            ]
        }]
    }));
    assert!(output.contains("_Chart data: Plan (radar)_"));
    assert!(output.contains("| Series | Revenue | Margin |"));
    assert!(output.contains("| Actual | 10 | 20 |"));
    assert!(output.contains("| Target | 12 | 18 |"));
}

#[test]
fn projects_hierarchy_charts_with_paths() {
    let output = project_chart(serde_json::json!({
        "series": [{
            "name": "Segments",
            "type": "treemap",
            "data": [{
                "name": "Region",
                "children": [
                    {"name": "EMEA", "children": [{"name": "UK", "value": 6}]},
                    {"name": "APAC", "value": 20}
                ]
            }]
        }]
    }));
    assert!(output.contains("_Chart data: Segments (treemap)_"));
    assert!(output.contains("| Path | Value |"));
    assert!(output.contains("| Region / EMEA / UK | 6 |"));
    assert!(output.contains("| Region / APAC | 20 |"));
}

#[test]
fn projects_multidimensional_series_dimensions() {
    let output = project_chart(serde_json::json!({
        "xAxis": {"data": ["2026-01", "2026-02"]},
        "series": [{
            "name": "OHLC",
            "type": "candlestick",
            "data": [[10, 12, 9, 13], [12, 13, 11, 14]]
        }]
    }));
    assert!(output.contains("| Category | Open | Close | Low | High |"));
    assert!(output.contains("| 2026-01 | 10 | 12 | 9 | 13 |"));

    let output = project_chart(serde_json::json!({
        "series": [{
            "name": "Heat",
            "type": "heatmap",
            "data": [[0, 0, 5], [0, 1, 8]]
        }]
    }));
    assert!(output.contains("| X | Y | Value |"));
    assert!(output.contains("| 0 | 1 | 8 |"));

    let output = project_chart(serde_json::json!({
        "series": [{
            "name": "Cloud",
            "type": "scatter",
            "data": [[1, 2, 3]]
        }]
    }));
    assert!(output.contains("| X | Y | Value 3 |"));
    assert!(output.contains("| 1 | 2 | 3 |"));
}

#[test]
fn section_projection_stops_at_same_level_heading() {
    let output = project_section(&sample(), "introduction", &MarkdownOptions::default()).unwrap();
    assert!(output.contains("block:intro"));
    assert!(output.contains("block:chart"));
    assert!(!output.contains("block:appendix"));

    // Match by exact heading block_id (`intro`) and by ordinal chapter label (`第一章`, `第2章`).
    let by_id = project_section(&sample(), "intro", &MarkdownOptions::default()).unwrap();
    assert!(by_id.contains("block:intro"));
    assert!(!by_id.contains("block:appendix"));

    let by_ordinal_1 = project_section(&sample(), "第一章", &MarkdownOptions::default()).unwrap();
    assert!(by_ordinal_1.contains("block:intro"));
    assert!(!by_ordinal_1.contains("block:appendix"));

    let by_ordinal_2 = project_section(&sample(), "第二章", &MarkdownOptions::default()).unwrap();
    assert!(by_ordinal_2.contains("block:appendix"));
    assert!(!by_ordinal_2.contains("block:intro"));

    // Truncated digest preview ending with `…` or `...` matches the full heading.
    let by_truncated_preview =
        project_section(&sample(), "Intro…", &MarkdownOptions::default()).unwrap();
    assert!(by_truncated_preview.contains("block:intro"));
}

#[test]
fn section_id_reads_the_requested_heading_and_text_ambiguity_fails_closed() {
    let mut doc = ReportDocument::new(doc_id("collision"));
    doc.blocks
        .push(Block::heading(block_id("intro"), 1, "Introduction"));
    doc.blocks
        .push(Block::paragraph(block_id("intro_body"), "Opening body."));
    doc.blocks.push(Block::heading(
        block_id("intro_appendix"),
        1,
        "Introduction appendix",
    ));
    doc.blocks.push(Block::paragraph(
        block_id("appendix_body"),
        "Appendix body.",
    ));

    let projector = MarkdownProjector::new(&doc, &MarkdownOptions::default());
    let by_id = projector
        .render_section_by_id(&block_id("intro_appendix"))
        .unwrap();
    assert!(by_id.contains("block:intro_appendix"));
    assert!(by_id.contains("block:appendix_body"));
    assert!(!by_id.contains("block:intro_body"));

    let error = project_section(&doc, "Introduction", &MarkdownOptions::default()).unwrap_err();
    assert!(matches!(error, MarkdownError::AmbiguousSection { .. }));
    assert!(error.to_string().contains("intro"));
    assert!(error.to_string().contains("intro_appendix"));

    // A block_ids read of a heading returns only that heading, never the section body.
    let heading_only = project_blocks(
        &doc,
        &[block_id("intro")],
        &MarkdownOptions {
            include_front_matter: false,
            ..Default::default()
        },
    )
    .unwrap();
    assert!(heading_only.contains("block:intro"));
    assert!(!heading_only.contains("block:intro_body"));
}

#[test]
fn block_projection_preserves_document_order_and_missing_ids_self_heal() {
    let output = project_blocks(
        &sample(),
        &[block_id("chart"), block_id("summary")],
        &MarkdownOptions {
            include_front_matter: false,
            ..Default::default()
        },
    )
    .unwrap();
    let summary_pos = output.find("block:summary").unwrap();
    let chart_pos = output.find("block:chart").unwrap();
    assert!(summary_pos < chart_pos);
    let error = project_blocks(
        &sample(),
        &[block_id("missing")],
        &MarkdownOptions::default(),
    )
    .unwrap_err();
    assert!(error.to_string().contains("Valid block_ids"));
    assert!(error.to_string().contains("summary"));
}

#[test]
fn missing_section_lists_headings() {
    let error = project_section(&sample(), "nope", &MarkdownOptions::default()).unwrap_err();
    assert!(matches!(error, MarkdownError::SectionNotFound { .. }));
    assert!(error.to_string().contains("Introduction"));
}
