use super::*;
use crate::test_support::*;
use crate::{
    block::{Block, BlockStyle, CellValue, ColumnNode, TableBlock},
    model::{ReportDocument, SourceEntry},
};
use std::collections::BTreeMap;

fn nested_table() -> TableBlock {
    TableBlock {
        title: Some("Revenue".into()),
        numbering: None,
        note: None,
        columns: vec![
            ColumnNode::group(
                "revenue",
                "Revenue",
                vec![
                    ColumnNode::leaf("q1", "Q1").with_type(ColumnDataType::Currency),
                    ColumnNode::leaf("q2", "Q2").with_type(ColumnDataType::Currency),
                ],
            ),
            ColumnNode::leaf("region", "Region"),
            ColumnNode::group(
                "margin",
                "Margin",
                vec![ColumnNode::group(
                    "gross",
                    "Gross",
                    vec![
                        ColumnNode::leaf("gm_q1", "Q1").with_type(ColumnDataType::Percent),
                        ColumnNode::leaf("gm_q2", "Q2").with_type(ColumnDataType::Percent),
                    ],
                )],
            ),
        ],
        rows: vec![
            BTreeMap::from([
                ("region".to_string(), CellValue::text("North America")),
                ("q1".to_string(), CellValue::Integer(120)),
                ("q2".to_string(), CellValue::Integer(150)),
                ("gm_q1".to_string(), CellValue::Float(62.5)),
                ("gm_q2".to_string(), CellValue::Float(64.0)),
            ]),
            BTreeMap::from([
                ("region".to_string(), CellValue::text("EMEA")),
                ("q1".to_string(), CellValue::Integer(80)),
                ("q2".to_string(), CellValue::Integer(95)),
                ("gm_q1".to_string(), CellValue::Float(58.0)),
                ("gm_q2".to_string(), CellValue::Float(60.0)),
            ]),
        ],
        summary_rows: vec![BTreeMap::from([
            ("region".to_string(), CellValue::text("Total")),
            ("q1".to_string(), CellValue::Integer(200)),
            ("q2".to_string(), CellValue::Integer(245)),
        ])],
    }
}

#[test]
fn computes_three_level_spans_and_exact_widths() {
    let layout = compute_table_layout(
        &block_id("financials"),
        &nested_table(),
        &BlockStyle::default(),
    )
    .unwrap();
    assert_eq!(layout.header_depth, 3);
    assert_eq!(layout.leaf_columns.len(), 5);
    assert_eq!(layout.header_rows.len(), 3);
    let row0 = &layout.header_rows[0];
    assert_eq!(
        row0.iter()
            .map(|cell| (cell.label.as_str(), cell.colspan, cell.rowspan))
            .collect::<Vec<_>>(),
        vec![("Revenue", 2, 1), ("Region", 1, 3), ("Margin", 2, 1)]
    );
    let row1 = &layout.header_rows[1];
    assert_eq!(
        row1.iter()
            .map(|cell| (cell.label.as_str(), cell.colspan, cell.rowspan))
            .collect::<Vec<_>>(),
        vec![("Q1", 1, 2), ("Q2", 1, 2), ("Gross", 2, 1)]
    );
    assert_eq!(
        layout.total_width_pct,
        100.0,
        "{:?}",
        layout
            .leaf_columns
            .iter()
            .map(|leaf| leaf.width_pct)
            .collect::<Vec<_>>()
    );
    assert!(layout.leaf_columns.iter().all(|leaf| leaf.width_pct > 0.0));
    assert!(
        layout
            .leaf_columns
            .iter()
            .all(|leaf| leaf.width_css.ends_with('%'))
    );
}

#[test]
fn p90_and_cjk_content_influence_widths() {
    let mut table = nested_table();
    table.rows[0].insert(
        "region".to_string(),
        CellValue::text("北美洲地区业务收入明细"),
    );
    let layout = compute_table_layout(&block_id("t"), &table, &BlockStyle::default()).unwrap();
    let region = layout
        .leaf_columns
        .iter()
        .find(|leaf| leaf.key == "region")
        .unwrap();
    assert!(region.width_pct > 10.0);
}

#[test]
fn explicit_widths_are_respected_and_normalized() {
    let style = BlockStyle {
        column_widths: Some(BTreeMap::from([
            ("region".to_string(), "120px".to_string()),
            ("q1".to_string(), "15%".to_string()),
        ])),
        ..Default::default()
    };
    let layout = compute_table_layout(&block_id("t"), &nested_table(), &style).unwrap();
    let region = layout
        .leaf_columns
        .iter()
        .find(|leaf| leaf.key == "region")
        .unwrap();
    assert!((region.width_pct - 12.5).abs() < 0.01);
    let q1 = layout
        .leaf_columns
        .iter()
        .find(|leaf| leaf.key == "q1")
        .unwrap();
    assert!((q1.width_pct - 15.0).abs() < 0.01);
    assert_eq!(layout.total_width_pct, 100.0);
}

#[test]
fn cjk_visual_width_widens_fitted_column() {
    let table = TableBlock {
        title: None,
        numbering: None,
        note: None,
        columns: vec![
            ColumnNode::leaf("cjk", "CJK"),
            ColumnNode::leaf("ascii", "ASCII"),
        ],
        rows: vec![BTreeMap::from([
            ("cjk".to_string(), CellValue::text("短期收入增长强劲")),
            ("ascii".to_string(), CellValue::text("shortrev")),
        ])],
        summary_rows: vec![],
    };
    let layout = compute_table_layout(&block_id("cjk"), &table, &BlockStyle::default()).unwrap();
    let cjk = layout
        .leaf_columns
        .iter()
        .find(|leaf| leaf.key == "cjk")
        .unwrap();
    let ascii = layout
        .leaf_columns
        .iter()
        .find(|leaf| leaf.key == "ascii")
        .unwrap();
    assert!(
        cjk.width_pct > ascii.width_pct,
        "CJK={} ASCII={}",
        cjk.width_pct,
        ascii.width_pct
    );
}

#[test]
fn p90_ignores_a_single_huge_outlier() {
    let mut outlier_rows = Vec::new();
    for index in 0..10 {
        let outlier = if index == 9 {
            "X".repeat(80)
        } else {
            "x".to_string()
        };
        outlier_rows.push(BTreeMap::from([
            ("outlier".to_string(), CellValue::text(outlier)),
            ("steady".to_string(), CellValue::text("steadyvalue")),
        ]));
    }
    let table = TableBlock {
        title: None,
        numbering: None,
        note: None,
        columns: vec![
            ColumnNode::leaf("outlier", "O"),
            ColumnNode::leaf("steady", "S"),
        ],
        rows: outlier_rows,
        summary_rows: vec![],
    };
    let layout = compute_table_layout(&block_id("p90"), &table, &BlockStyle::default()).unwrap();
    let outlier = layout
        .leaf_columns
        .iter()
        .find(|leaf| leaf.key == "outlier")
        .unwrap();
    let steady = layout
        .leaf_columns
        .iter()
        .find(|leaf| leaf.key == "steady")
        .unwrap();
    assert!(
        outlier.width_pct < steady.width_pct,
        "P90 should ignore the 80-char outlier: outlier={} steady={}",
        outlier.width_pct,
        steady.width_pct
    );
}

#[test]
fn citations_number_across_prose_tables_and_charts() {
    let mut doc = ReportDocument::new(doc_id("doc"));
    doc.sources
        .insert(source_id("a"), SourceEntry::new("Prose source"));
    doc.sources
        .insert(source_id("b"), SourceEntry::new("Table source"));
    doc.sources
        .insert(source_id("c"), SourceEntry::new("Chart source"));
    doc.blocks
        .push(Block::paragraph(block_id("p"), "Hello [^a]"));
    doc.blocks.push(
        Block::table(
            block_id("t"),
            TableBlock {
                title: None,
                numbering: None,
                note: None,
                columns: vec![ColumnNode::leaf("metric", "Metric")],
                rows: vec![],
                summary_rows: vec![],
            },
        )
        .with_sources([source_id("b")]),
    );
    doc.blocks.push(
        Block::new(
            block_id("chart"),
            crate::block::BlockKind::Echarts(crate::block::EchartsBlock {
                title: None,
                height: 240,
                option: serde_json::json!({"series": [{"type": "bar", "data": [1]}]}),
                option_patch: None,
                fallback_summary: "Chart summary".into(),
                numbering: None,
                note: None,
            }),
        )
        .with_sources([source_id("c")]),
    );
    let meta = compute_document_meta(&doc);
    assert_eq!(
        meta.citation_map.get("a").map(|entry| entry.number),
        Some(1)
    );
    assert_eq!(
        meta.citation_map.get("b").map(|entry| entry.number),
        Some(2)
    );
    assert_eq!(
        meta.citation_map.get("c").map(|entry| entry.number),
        Some(3)
    );
    assert_eq!(
        meta.bibliography
            .iter()
            .map(|entry| entry.source_id.clone())
            .collect::<Vec<_>>(),
        vec![source_id("a"), source_id("b"), source_id("c")]
    );
}

#[test]
fn citation_map_and_bibliography_follow_first_appearance() {
    let mut doc = ReportDocument::new(doc_id("doc"));
    doc.sources
        .insert(source_id("b"), SourceEntry::new("Second"));
    doc.sources
        .insert(source_id("a"), SourceEntry::new("First"));
    doc.blocks
        .push(Block::paragraph(block_id("p1"), "Hello [^b]"));
    doc.blocks
        .push(Block::paragraph(block_id("p2"), "World [^a] [^b]"));
    let meta = compute_document_meta(&doc);
    assert_eq!(meta.citation_map["b"].number, 1);
    assert_eq!(meta.citation_map["a"].number, 2);
    assert_eq!(meta.bibliography.len(), 2);
    assert_eq!(meta.citation_map["a"].title, "First");
}

#[test]
fn block_framing_labels_and_attribution_are_deterministic() {
    let mut doc = ReportDocument::new(doc_id("framing"));
    doc.sources.insert(
        source_id("sec10"),
        SourceEntry {
            authors: vec!["Jane Smith".into()],
            date: Some("2025-01-01".into()),
            ..SourceEntry::new("Annual filing")
        },
    );
    doc.references.style = crate::model::CitationStyle::Numeric;
    doc.blocks.push(
        Block::table(
            block_id("t1"),
            TableBlock {
                title: Some("Revenue".into()),
                numbering: None,
                note: Some("Audited".into()),
                columns: vec![ColumnNode::leaf("region", "Region")],
                rows: vec![],
                summary_rows: vec![],
            },
        )
        .with_sources([source_id("sec10")]),
    );
    if let Some(block) = doc.blocks.last_mut() {
        block.source_note = Some("Internal filing".into());
    }
    doc.blocks.push(Block::new(
        block_id("chart"),
        BlockKind::Echarts(crate::block::EchartsBlock {
            title: Some("Trend".into()),
            height: 300,
            option: serde_json::json!({"series": [{"type": "line", "data": [1]}]}),
            option_patch: None,
            fallback_summary: "Upward trend".into(),
            numbering: None,
            note: None,
        }),
    ));
    doc.blocks.push(Block::new(
        block_id("card"),
        BlockKind::CustomHtml(crate::block::CustomHtmlBlock {
            title: Some("KPI".into()),
            numbering: Some(NumberingKind::Figure),
            note: None,
            html: "<div>18%</div>".into(),
            css: None,
            fallback_text: "18%".into(),
        }),
    ));
    let meta = compute_document_meta(&doc);
    assert_eq!(meta.block_framing[&block_id("t1")].label, "表 1");
    assert_eq!(meta.block_framing[&block_id("chart")].label, "图 1");
    assert_eq!(meta.block_framing[&block_id("card")].label, "图 2");
    let attribution = meta.block_framing[&block_id("t1")]
        .attribution
        .as_deref()
        .unwrap();
    assert!(attribution.contains("资料来源：Internal filing [1]"));
    assert!(attribution.contains("注：Audited"));
}

#[test]
fn chart_requirements_collect_maps_and_extensions() {
    let mut doc = ReportDocument::new(doc_id("charts"));
    doc.blocks.push(Block::new(
        block_id("maps"),
        BlockKind::Echarts(crate::block::EchartsBlock {
            title: None,
            height: 300,
            option: serde_json::json!({
                "geo": [{"map": "china"}],
                "series": [
                    {"type": "map", "map": "world", "data": [1]},
                    {"type": "wordCloud", "data": [1]},
                    {"type": "bar3D", "data": [1]}
                ]
            }),
            option_patch: None,
            fallback_summary: "Maps and extensions.".into(),
            numbering: None,
            note: None,
        }),
    ));
    doc.blocks.push(Block::new(
        block_id("plain"),
        BlockKind::Echarts(crate::block::EchartsBlock {
            title: None,
            height: 300,
            option: serde_json::json!({
                "series": [{"type": "bar", "data": [1]}]
            }),
            option_patch: None,
            fallback_summary: "Plain chart.".into(),
            numbering: None,
            note: None,
        }),
    ));
    let meta = compute_document_meta(&doc);
    let requirements = meta
        .chart_requirements
        .get(&block_id("maps"))
        .expect("map requirements");
    assert!(requirements.required_maps.contains("china"));
    assert!(requirements.required_maps.contains("world"));
    assert!(requirements.required_extensions.contains("wordCloud"));
    assert!(requirements.required_extensions.contains("bar3D"));
    assert!(!meta.chart_requirements.contains_key(&block_id("plain")));
}

#[test]
fn chart_tables_match_markdown_projection() {
    let mut doc = ReportDocument::new(doc_id("charts"));
    doc.blocks.push(Block::new(
        block_id("chart"),
        BlockKind::Echarts(crate::block::EchartsBlock {
            title: Some("Quarterly revenue".into()),
            height: 300,
            option: serde_json::json!({
                "xAxis": {"type": "category", "data": ["Q1", "Q2"]},
                "series": [{"name": "Revenue", "type": "bar", "data": [120, 150]}]
            }),
            option_patch: None,
            fallback_summary: "Revenue grew.".into(),
            numbering: None,
            note: None,
        }),
    ));
    let meta = compute_document_meta(&doc);
    let tables = meta
        .chart_tables
        .get(&block_id("chart"))
        .expect("chart table");
    assert_eq!(tables.len(), 1);
    assert_eq!(tables[0].title, "Revenue (bar)");
    assert_eq!(
        tables[0].headers,
        vec!["Category".to_string(), "Value".to_string()]
    );
    assert_eq!(
        tables[0].rows,
        vec![
            vec!["Q1".to_string(), "120".to_string()],
            vec!["Q2".to_string(), "150".to_string()]
        ]
    );

    let markdown = crate::project_markdown(&doc, &crate::MarkdownOptions::default()).unwrap();
    assert!(markdown.contains("_Chart data: Revenue (bar)_"));
    assert!(markdown.contains("| Q1 | 120 |"));
    assert!(markdown.contains("| Q2 | 150 |"));
}

#[test]
fn toc_tree_is_nested_and_depth_filtered() {
    let mut doc = ReportDocument::new(doc_id("doc"));
    doc.toc.max_depth = 2;
    doc.blocks
        .push(Block::heading(block_id("intro"), 1, "Introduction"));
    doc.blocks
        .push(Block::heading(block_id("context"), 2, "Context"));
    doc.blocks.push(Block::heading(block_id("deep"), 3, "Deep"));
    doc.blocks.push(Block::heading(block_id("next"), 2, "Next"));
    doc.blocks
        .push(Block::heading(block_id("appendix"), 1, "Appendix"));
    let tree = compute_toc_tree(&doc);
    assert_eq!(tree.len(), 2);
    assert_eq!(tree[0].text, "Introduction");
    assert_eq!(tree[0].number, "1");
    assert_eq!(tree[0].children.len(), 2);
    assert_eq!(tree[0].children[1].number, "1.2");
    assert_eq!(tree[1].text, "Appendix");
}
