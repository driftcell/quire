use super::{wire::RawPatchOp, *};
use crate::test_support::*;
use crate::{
    block::{Block, ColumnNode, EchartsBlock, TableBlock},
    model::ReportDocument,
};

fn doc() -> ReportDocument {
    let mut doc = ReportDocument::new(doc_id("doc"));
    doc.blocks.push(Block::paragraph(block_id("a"), "Alpha"));
    doc.blocks.push(Block::paragraph(block_id("b"), "Beta"));
    doc.blocks.push(Block::paragraph(block_id("c"), "Gamma"));
    doc
}

fn ids(doc: &ReportDocument) -> Vec<BlockId> {
    doc.blocks.iter().map(|block| block.id.clone()).collect()
}

#[test]
fn inserts_append_replace_delete_and_move() {
    let mut doc = doc();
    let outcome = apply_operations_in_place(
        &mut doc,
        &[
            PatchOp::InsertAfter {
                target_id: block_id("a"),
                blocks: vec![Block::paragraph(block_id("a2"), "After A")],
            },
            PatchOp::InsertBefore {
                target_id: block_id("c"),
                blocks: vec![Block::paragraph(block_id("bc"), "Before C")],
            },
            PatchOp::Append {
                blocks: vec![Block::paragraph(block_id("z"), "Zulu")],
            },
            PatchOp::ReplaceRange {
                start_id: block_id("b"),
                end_id: block_id("b"),
                blocks: vec![Block::paragraph(block_id("b-new"), "Beta replaced")],
            },
            PatchOp::DeleteBlocks {
                block_ids: vec![block_id("bc")],
            },
            PatchOp::MoveBlock {
                block_id: block_id("z"),
                after_id: Some(block_id("a")),
            },
        ],
    )
    .unwrap();
    assert_eq!(
        ids(&doc),
        vec![
            block_id("a"),
            block_id("z"),
            block_id("a2"),
            block_id("b-new"),
            block_id("c")
        ]
    );
    assert_eq!(
        outcome.inserted_block_ids,
        vec![
            block_id("a2"),
            block_id("bc"),
            block_id("z"),
            block_id("b-new")
        ]
    );
    assert!(outcome.deleted_block_ids.contains(&block_id("bc")));
    assert_eq!(outcome.moved_block_ids, vec![block_id("z")]);
    assert_eq!(outcome.id_mapping.get("a2"), Some(&block_id("a2")));
}

#[test]
fn failed_batch_is_atomic() {
    let mut doc = doc();
    let result = apply_operations_in_place(
        &mut doc,
        &[
            PatchOp::DeleteBlocks {
                block_ids: vec![block_id("a")],
            },
            PatchOp::PatchBlock {
                block_id: block_id("missing"),
                patch: Box::new(BlockPatch {
                    text: Some("x".into()),
                    ..Default::default()
                }),
            },
        ],
    );
    assert!(matches!(result, Err(PatchError::BlockNotFound { .. })));
    assert_eq!(ids(&doc), vec![block_id("a"), block_id("b"), block_id("c")]);
}

#[test]
fn move_to_front_and_duplicate_validation() {
    let mut doc = doc();
    apply_operations_in_place(
        &mut doc,
        &[PatchOp::MoveBlock {
            block_id: block_id("c"),
            after_id: None,
        }],
    )
    .unwrap();
    assert_eq!(ids(&doc), vec![block_id("c"), block_id("a"), block_id("b")]);
    let outcome = apply_operations_in_place(
        &mut doc,
        &[PatchOp::Append {
            blocks: vec![Block::paragraph(block_id("a"), "duplicate")],
        }],
    )
    .unwrap();
    assert_eq!(outcome.assigned_ids.len(), 1);
    assert_eq!(
        outcome.assigned_ids[0].requested.as_ref(),
        Some(&block_id("a"))
    );
    assert_eq!(ids(&doc).len(), 4);
}

#[test]
fn patch_block_text_style_and_sources() {
    let mut doc = doc();
    apply_operations_in_place(
        &mut doc,
        &[PatchOp::PatchBlock {
            block_id: block_id("a"),
            patch: Box::new(BlockPatch {
                text: Some("Alpha updated **bold**".into()),
                class_name: Some("lead".into()),
                source_note: Some("Primary filing".into()),
                source_ids: Some(vec![source_id("sec10")]),
                style: Some(BlockStylePatch {
                    keep_together: Some(true),
                    variant: Some("callout".into()),
                    ..Default::default()
                }),
                ..Default::default()
            }),
        }],
    )
    .unwrap();
    let block = doc.block("a").unwrap();
    assert_eq!(block.visible_text(), "Alpha updated **bold**");
    assert_eq!(block.class_name.as_deref(), Some("lead"));
    assert!(block.style.keep_together);
    assert_eq!(block.source_ids, vec!["sec10"]);
}

#[test]
fn patch_block_accepts_flattened_and_hybrid_fields() {
    let flattened: PatchOp = serde_json::from_value(serde_json::json!({
        "type": "patch_block",
        "block_id": "v1-overview",
        "title": "Flattened title",
        "lint_ignore": ["QR201"]
    }))
    .expect("flattened patch_block must deserialize");
    let PatchOp::PatchBlock { block_id, patch } = flattened else {
        panic!("expected PatchBlock");
    };
    assert_eq!(block_id.as_str(), "v1-overview");
    assert_eq!(patch.title.as_deref(), Some("Flattened title"));
    assert_eq!(patch.lint_ignore, Some(vec!["QR201".to_string()]));

    let hybrid: PatchOp = serde_json::from_value(serde_json::json!({
        "type": "patch_block",
        "block_id": "v8-industry-map",
        "lint_ignore": ["QR204"],
        "patch": {"note": "Updated note"}
    }))
    .expect("hybrid patch_block must merge top-level and nested patch fields");
    let PatchOp::PatchBlock { patch, .. } = hybrid else {
        panic!("expected PatchBlock");
    };
    assert_eq!(patch.lint_ignore, Some(vec!["QR204".to_string()]));
    assert_eq!(patch.note.as_deref(), Some("Updated note"));
}

#[test]
fn table_append_update_cell_and_in_place_columns_rows_patch() {
    let mut doc = ReportDocument::new(doc_id("doc"));
    doc.blocks.push(Block::table(
        block_id("t"),
        TableBlock {
            title: None,
            numbering: None,
            note: None,
            columns: vec![
                ColumnNode::leaf("region", "Region"),
                ColumnNode::leaf("revenue", "Revenue"),
            ],
            rows: vec![BTreeMap::from([
                ("region".to_string(), CellValue::text("APAC")),
                ("revenue".to_string(), CellValue::Integer(10)),
            ])],
            summary_rows: vec![],
        },
    ));
    apply_operations_in_place(
        &mut doc,
        &[PatchOp::PatchBlock {
            block_id: block_id("t"),
            patch: Box::new(BlockPatch {
                title: Some("Revenue".into()),
                numbering: Some(NumberingKind::Table),
                note: Some("Audited".into()),
                columns: Some(vec![
                    ColumnNode::leaf("region", "Region"),
                    ColumnNode::leaf("revenue", "Revenue ($M)"),
                ]),
                append_rows: vec![BTreeMap::from([
                    ("region".to_string(), CellValue::text("EMEA")),
                    ("revenue".to_string(), CellValue::Integer(20)),
                ])],
                update_cell: Some(CellUpdate {
                    row_index: 0,
                    column_key: "revenue".into(),
                    value: CellValue::Integer(15),
                    summary: false,
                }),
                ..Default::default()
            }),
        }],
    )
    .unwrap();
    let BlockKind::Table(table) = &doc.block("t").unwrap().body else {
        panic!()
    };
    assert_eq!(table.columns[1].label, "Revenue ($M)");
    assert_eq!(table.rows.len(), 2);
    assert_eq!(table.rows[0]["revenue"], CellValue::Integer(15));
    assert_eq!(table.title.as_deref(), Some("Revenue"));
    assert_eq!(table.numbering, Some(NumberingKind::Table));
    assert_eq!(table.note.as_deref(), Some("Audited"));

    let legacy: BlockPatch =
        serde_json::from_value(serde_json::json!({"caption": "Legacy title"})).unwrap();
    assert_eq!(legacy.title.as_deref(), Some("Legacy title"));
}

#[test]
fn echarts_option_patch_deep_merges() {
    let mut doc = ReportDocument::new(doc_id("doc"));
    doc.blocks.push(Block::new(
        block_id("chart"),
        BlockKind::Echarts(EchartsBlock {
            title: None,
            height: 300,
            option: serde_json::json!({"series":[{"type":"bar","data":[1]}]}),
            option_patch: Some(serde_json::json!({"series":[{"type":"bar"}]})),
            fallback_summary: "Summary".into(),
            numbering: None,
            note: None,
        }),
    ));
    apply_operations_in_place(
        &mut doc,
        &[PatchOp::PatchBlock {
            block_id: block_id("chart"),
            patch: Box::new(BlockPatch {
                option_patch: Some(serde_json::json!({"series":[{"data":[1,2]}]})),
                fallback_summary: Some("Updated summary".into()),
                ..Default::default()
            }),
        }],
    )
    .unwrap();
    let BlockKind::Echarts(chart) = &doc.block("chart").unwrap().body else {
        panic!()
    };
    assert_eq!(
        chart.option_patch.as_ref().unwrap()["series"][0]["data"],
        serde_json::json!([1, 2])
    );
    assert_eq!(
        chart.option_patch.as_ref().unwrap()["series"][0]["type"],
        serde_json::json!("bar")
    );
}

#[test]
fn echarts_patch_replaces_option_and_clears_patch() {
    let mut doc = ReportDocument::new(doc_id("doc"));
    doc.blocks.push(Block::new(
        block_id("chart"),
        BlockKind::Echarts(EchartsBlock {
            title: None,
            height: 300,
            option: serde_json::json!({"series": [{"type": "bar", "data": [1]}]}),
            option_patch: Some(serde_json::json!({"series": [{"type": "bar"}]})),
            fallback_summary: "Summary".into(),
            numbering: None,
            note: None,
        }),
    ));
    apply_operations_in_place(
        &mut doc,
        &[PatchOp::PatchBlock {
            block_id: block_id("chart"),
            patch: Box::new(BlockPatch {
                option: Some(serde_json::json!({
                    "dataset": {"source": [["Q", "V"], ["Q1", 10]]},
                    "series": [{"type": "bar", "data": [10]}]
                })),
                height: Some(480),
                title: Some("Patched chart".into()),
                ..Default::default()
            }),
        }],
    )
    .unwrap();
    let BlockKind::Echarts(chart) = &doc.block("chart").unwrap().body else {
        panic!()
    };
    assert_eq!(chart.height, 480);
    assert!(chart.option_patch.is_none());
    assert_eq!(chart.title.as_deref(), Some("Patched chart"));
    assert_eq!(
        chart.option["dataset"]["source"][1][1],
        serde_json::json!(10)
    );

    let error = apply_operations_in_place(
        &mut doc,
        &[PatchOp::PatchBlock {
            block_id: block_id("chart"),
            patch: Box::new(BlockPatch {
                option: Some(serde_json::json!("not an object")),
                ..Default::default()
            }),
        }],
    )
    .unwrap_err();
    assert!(matches!(error, PatchError::InvalidPatch { .. }));
}

#[test]
fn unsupported_patch_lists_supported_fields() {
    let mut doc = doc();
    let error = apply_operations_in_place(
        &mut doc,
        &[PatchOp::PatchBlock {
            block_id: block_id("a"),
            patch: Box::new(BlockPatch {
                update_cell: Some(CellUpdate {
                    row_index: 0,
                    column_key: "x".into(),
                    value: CellValue::Null,
                    summary: false,
                }),
                ..Default::default()
            }),
        }],
    )
    .unwrap_err();
    match error {
        PatchError::UnsupportedPatch {
            fields, supported, ..
        } => {
            assert!(fields.contains("update_cell"));
            assert!(supported.contains("text"));
        }
        other => panic!("expected unsupported patch error, got {other:?}"),
    }
}

#[test]
fn patches_reject_unknown_fields_and_keep_caption_alias() {
    let patch: BlockPatch = serde_json::from_value(serde_json::json!({"caption": "Legacy title"}))
        .expect("caption alias");
    assert_eq!(patch.title.as_deref(), Some("Legacy title"));

    let error = serde_json::from_value::<BlockPatch>(serde_json::json!({"titel": "typo"}))
        .expect_err("unknown block patch field");
    assert!(error.to_string().contains("titel"), "{error}");

    let error =
        serde_json::from_value::<BlockStylePatch>(serde_json::json!({"page_break_befor": true}))
            .expect_err("unknown style field");
    assert!(error.to_string().contains("page_break_befor"), "{error}");

    let error = serde_json::from_value::<CellUpdate>(serde_json::json!({
        "row_index": 0,
        "column_key": "q1",
        "value": 1,
        "sumary": true
    }))
    .expect_err("unknown cell update field");
    assert!(error.to_string().contains("sumary"), "{error}");
}

#[test]
fn touched_ids_include_anchors_and_new_blocks() {
    let ops = PatchOp::InsertAfter {
        target_id: block_id("a"),
        blocks: vec![Block::paragraph(block_id("new"), "x")],
    };
    let touched = ops.touched_block_ids();
    assert!(touched.contains("a"));
    assert!(touched.contains("new"));
}

#[test]
fn append_to_section_stops_before_same_or_higher_heading() {
    let mut doc = ReportDocument::new(doc_id("doc"));
    doc.blocks
        .push(Block::heading(block_id("intro"), 1, "Intro"));
    doc.blocks
        .push(Block::paragraph(block_id("intro-body"), "Body"));
    doc.blocks.push(Block::heading(block_id("next"), 1, "Next"));
    doc.blocks.push(Block::heading(block_id("sub"), 2, "Sub"));
    apply_operations_in_place(
        &mut doc,
        &[PatchOp::AppendToSection {
            section_id: block_id("intro"),
            blocks: vec![Block::paragraph(block_id("added"), "Added")],
        }],
    )
    .unwrap();
    let ids: Vec<&str> = doc.blocks.iter().map(|block| block.id.as_str()).collect();
    assert_eq!(ids, vec!["intro", "intro-body", "added", "next", "sub"]);
}

#[test]
fn empty_or_colliding_ids_are_auto_assigned_and_reported() {
    let mut doc = ReportDocument::new(doc_id("doc"));
    doc.blocks
        .push(Block::paragraph(block_id("existing"), "Existing"));
    let pending: Block = serde_json::from_value(serde_json::json!({
        "id": "",
        "type": "paragraph",
        "text": "Pending"
    }))
    .unwrap();
    let outcome = apply_operations_in_place(
        &mut doc,
        &[PatchOp::Append {
            blocks: vec![pending, Block::paragraph(block_id("existing"), "Collision")],
        }],
    )
    .unwrap();
    assert_eq!(outcome.assigned_ids.len(), 2);
    assert_eq!(outcome.assigned_ids[0].requested, None);
    assert_eq!(
        outcome.assigned_ids[1].requested.as_ref(),
        Some(&block_id("existing"))
    );
    assert_eq!(doc.blocks.len(), 3);
    assert_eq!(
        outcome.applied_ops,
        [PatchOp::Append {
            blocks: doc.blocks[1..].to_vec()
        }]
    );
}

#[test]
fn patch_operation_stack_size_is_bounded() {
    assert!(
        std::mem::size_of::<PatchOp>() <= 96,
        "PatchOp size is {}; cover={} toc={} refs={} theme={} sources={} block={}",
        std::mem::size_of::<PatchOp>(),
        std::mem::size_of::<CoverConfig>(),
        std::mem::size_of::<TocConfig>(),
        std::mem::size_of::<ReferencesConfig>(),
        std::mem::size_of::<ThemeConfig>(),
        std::mem::size_of::<IndexMap<SourceId, SourceEntry>>(),
        std::mem::size_of::<Block>()
    );
    assert!(
        std::mem::size_of::<RawPatchOp>() <= 96,
        "RawPatchOp size is {}",
        std::mem::size_of::<RawPatchOp>()
    );
}
