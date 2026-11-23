use super::*;
use crate::test_support::*;

#[test]
fn block_serializes_with_discriminator_and_metadata() {
    let block = Block::paragraph(block_id("exec-summary"), "Revenue grew **18%** [^sec10]")
        .with_sources([source_id("sec10")]);
    let value = serde_json::to_value(&block).unwrap();
    assert_eq!(value["id"], "exec-summary");
    assert_eq!(value["type"], "paragraph");
    assert_eq!(value["text"], "Revenue grew **18%** [^sec10]");
    assert_eq!(value["source_ids"][0], "sec10");
}

#[test]
fn rich_and_primitive_cells_round_trip() {
    let rich: CellValue = serde_json::from_value(serde_json::json!({
        "text": "n/a",
        "class_name": "muted"
    }))
    .unwrap();
    assert_eq!(rich.display_text(), "n/a");
    assert_eq!(rich.rich_class(), Some("muted"));

    let number: CellValue = serde_json::from_value(serde_json::json!(12.5)).unwrap();
    assert_eq!(number.display_text(), "12.5");
}

#[test]
fn number_format_is_stable() {
    assert_eq!(format_number(1.0), "1");
    assert_eq!(format_number(1.25), "1.25");
    assert_eq!(format_number(-0.5), "-0.5");
}

#[test]
fn semantic_ids() {
    assert!(is_semantic_block_id("q3-revenue"));
    assert!(!is_semantic_block_id(""));
    assert!(!is_semantic_block_id("has space"));
    assert!(looks_like_uuid_v4("123e4567-e89b-42d3-a456-426614174000"));
    assert!(!looks_like_uuid_v4("123e4567-e89b-12d3-a456-426614174000"));
}

#[test]
fn table_shorthand_infers_columns_and_types() {
    let value = serde_json::json!({
        "id": "financials",
        "type": "table",
        "title": "Revenue",
        "headers": ["Region", "Revenue", "Growth", "Reported"],
        "rows": [
            ["APAC", "$1,200", "12.5%", "2026-01-15"],
            ["EMEA", "$900", "9.0%", "2026-02-01"]
        ],
        "summary_rows": [["Total", "$2,100", "10.8%", "2026-02-01"]]
    });
    let block: Block = serde_json::from_value(value).expect("shorthand table");
    let BlockKind::Table(table) = &block.body else {
        panic!("expected table");
    };
    assert_eq!(table.title.as_deref(), Some("Revenue"));
    assert_eq!(table.columns.len(), 4);
    assert_eq!(table.columns[0].data_type, Some(ColumnDataType::Text));
    assert_eq!(table.columns[1].data_type, Some(ColumnDataType::Currency));
    assert_eq!(table.columns[2].data_type, Some(ColumnDataType::Percent));
    assert_eq!(table.rows.len(), 2);
    assert_eq!(table.summary_rows.len(), 1);
    assert_eq!(
        table.rows[0].get("region").map(CellValue::display_text),
        Some("APAC".to_string())
    );
}

#[test]
fn table_caption_alias_and_numbering_defaults() {
    let value = serde_json::json!({
        "id": "t",
        "type": "table",
        "caption": "Legacy caption",
        "headers": ["A"],
        "rows": [["1"]]
    });
    let block: Block = serde_json::from_value(value).unwrap();
    let BlockKind::Table(table) = block.body else {
        panic!("table");
    };
    assert_eq!(table.title.as_deref(), Some("Legacy caption"));
    assert_eq!(table.numbering_kind(), NumberingKind::Table);
}

#[test]
fn empty_block_id_deserializes_to_pending_placeholder() {
    let block: Block = serde_json::from_value(serde_json::json!({
        "id": "",
        "type": "paragraph",
        "text": "hello"
    }))
    .unwrap();
    assert!(block.id.is_pending());
}

#[test]
fn table_schema_exposes_columns_and_headers_shapes() {
    let schema =
        serde_json::to_value(schemars::schema_for!(TableBlock)).expect("table schema serializes");
    let text = serde_json::to_string(&schema).expect("schema text");
    assert!(text.contains("\"columns\""), "{text}");
    assert!(text.contains("\"headers\""), "{text}");
    assert!(text.contains("\"oneOf\""), "{text}");
    assert!(text.contains("\"summary_rows\""), "{text}");
}

#[test]
fn echarts_requires_native_option_and_accepts_caption_alias() {
    let missing = serde_json::from_value::<Block>(serde_json::json!({
        "id": "chart",
        "type": "echarts",
        "fallback_summary": "Two points"
    }));
    assert!(missing.is_err(), "option must be required");

    let legacy_mode = serde_json::from_value::<Block>(serde_json::json!({
        "id": "chart",
        "type": "echarts",
        "mode": "option",
        "option": {"series": [{"type": "bar", "data": [1]}]},
        "fallback_summary": "One point"
    }));
    assert!(
        legacy_mode.is_err(),
        "removed chart mode/preset fields must not be accepted"
    );

    let legacy: Block = serde_json::from_value(serde_json::json!({
        "id": "chart",
        "type": "echarts",
        "caption": "Legacy caption",
        "option": {"series": [{"type": "bar", "data": [1]}]},
        "fallback_summary": "One point"
    }))
    .unwrap();
    let BlockKind::Echarts(legacy) = legacy.body else {
        panic!("chart");
    };
    assert_eq!(legacy.title.as_deref(), Some("Legacy caption"));
    assert_eq!(legacy.option["series"][0]["data"], serde_json::json!([1]));
}
