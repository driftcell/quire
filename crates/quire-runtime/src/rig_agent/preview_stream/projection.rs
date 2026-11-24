use super::*;

pub(crate) fn preview_id_for(streamer: &ToolPreviewStreamer) -> String {
    streamer.preview_id().to_string()
}

pub(crate) fn extract_preview_blocks(arguments: &Value) -> Vec<PreviewBlock> {
    let mut blocks = Vec::new();
    let mut seen = BTreeSet::new();
    collect_preview_blocks(
        arguments,
        0,
        &AnchorContext {
            anchor: PreviewAnchor::AppendToDoc,
            resolved: false,
        },
        &mut blocks,
        &mut seen,
    );
    blocks
}

fn parse_preview_block_id(raw: Option<&str>) -> Option<BlockId> {
    raw.map(str::trim)
        .filter(|raw| !raw.is_empty())
        .and_then(|raw| BlockId::parse(raw.to_string()).ok())
}

fn operation_type(object: &serde_json::Map<String, Value>) -> Option<&str> {
    object.get("type").and_then(Value::as_str)
}

fn is_patch_operation(op_type: Option<&str>, object: &serde_json::Map<String, Value>) -> bool {
    op_type == Some("patch_block")
        || (op_type.is_none() && object.contains_key("patch") && object.contains_key("block_id"))
}

fn infer_operation_anchor(
    object: &serde_json::Map<String, Value>,
    inherited: &AnchorContext,
) -> AnchorContext {
    let op_type = operation_type(object);
    if op_type == Some("append") {
        return AnchorContext {
            anchor: PreviewAnchor::AppendToDoc,
            resolved: true,
        };
    }
    if op_type == Some("append_to_section") || object.contains_key("section_id") {
        if let Some(section_id) =
            parse_preview_block_id(object.get("section_id").and_then(Value::as_str))
        {
            return AnchorContext {
                anchor: PreviewAnchor::AppendToSection { section_id },
                resolved: true,
            };
        }
        return AnchorContext {
            anchor: inherited.anchor.clone(),
            resolved: false,
        };
    }
    if op_type == Some("insert_after") {
        if let Some(block_id) =
            parse_preview_block_id(object.get("target_id").and_then(Value::as_str))
        {
            return AnchorContext {
                anchor: PreviewAnchor::AfterBlock { block_id },
                resolved: true,
            };
        }
        return AnchorContext {
            anchor: inherited.anchor.clone(),
            resolved: false,
        };
    }
    if op_type == Some("insert_before") {
        if let Some(block_id) =
            parse_preview_block_id(object.get("target_id").and_then(Value::as_str))
        {
            return AnchorContext {
                anchor: PreviewAnchor::BeforeBlock { block_id },
                resolved: true,
            };
        }
        return AnchorContext {
            anchor: inherited.anchor.clone(),
            resolved: false,
        };
    }
    if is_patch_operation(op_type, object) {
        if let Some(block_id) =
            parse_preview_block_id(object.get("block_id").and_then(Value::as_str))
        {
            return AnchorContext {
                anchor: PreviewAnchor::ReplaceBlock { block_id },
                resolved: true,
            };
        }
        return AnchorContext {
            anchor: inherited.anchor.clone(),
            resolved: false,
        };
    }
    if op_type == Some("replace_range") {
        if let Some(block_id) =
            parse_preview_block_id(object.get("start_id").and_then(Value::as_str))
        {
            return AnchorContext {
                anchor: PreviewAnchor::ReplaceBlock { block_id },
                resolved: true,
            };
        }
        return AnchorContext {
            anchor: inherited.anchor.clone(),
            resolved: false,
        };
    }
    inherited.clone()
}

fn collect_preview_blocks(
    value: &Value,
    depth: usize,
    anchor: &AnchorContext,
    blocks: &mut Vec<PreviewBlock>,
    seen: &mut BTreeSet<String>,
) {
    if depth > 8 {
        return;
    }
    match value {
        Value::Object(object) => {
            let op_type = operation_type(object);
            let next_anchor = infer_operation_anchor(object, anchor);
            let patch_op = is_patch_operation(op_type, object);
            let mut consumed_patch = false;
            if patch_op && let Some(Value::Object(patch)) = object.get("patch") {
                let mut merged = patch.clone();
                if !merged.contains_key("id")
                    && let Some(block_id) = object.get("block_id")
                {
                    merged.insert("id".to_string(), block_id.clone());
                }
                if let Some(block) = preview_block(&merged, &next_anchor) {
                    push_preview_block(block, blocks, seen);
                }
                consumed_patch = true;
            }
            let is_container =
                object.contains_key("ops") || object.contains_key("blocks") || patch_op;
            if !is_container && let Some(block) = preview_block(object, &next_anchor) {
                push_preview_block(block, blocks, seen);
                return;
            }
            let chained_insert = matches!(op_type, Some("insert_after" | "insert_before"));
            for (key, child) in object {
                if key == "patch" && consumed_patch {
                    continue;
                }
                // Never recurse into block-internal payload structures as if they were
                // child document blocks (e.g., ECharts `option.title.text` or table cells).
                if matches!(
                    key.as_str(),
                    "option"
                        | "option_patch"
                        | "rows"
                        | "columns"
                        | "sources"
                        | "source_ids"
                        | "numbering"
                ) {
                    continue;
                }
                if chained_insert && key == "blocks" {
                    collect_chained_insert_blocks(child, depth + 1, &next_anchor, blocks, seen);
                    continue;
                }
                collect_preview_blocks(child, depth + 1, &next_anchor, blocks, seen);
            }
        }
        Value::Array(values) => {
            for child in values {
                collect_preview_blocks(child, depth + 1, anchor, blocks, seen);
            }
        }
        _ => {}
    }
}

fn collect_chained_insert_blocks(
    value: &Value,
    depth: usize,
    anchor: &AnchorContext,
    blocks: &mut Vec<PreviewBlock>,
    seen: &mut BTreeSet<String>,
) {
    if depth > 8 {
        return;
    }
    let Value::Array(values) = value else {
        collect_preview_blocks(value, depth, anchor, blocks, seen);
        return;
    };
    let mut chain_anchor = anchor.clone();
    for child in values {
        collect_preview_blocks(child, depth + 1, &chain_anchor, blocks, seen);
        let previous_id = child
            .as_object()
            .and_then(|object| object.get("id"))
            .and_then(Value::as_str);
        if let Some(block_id) = parse_preview_block_id(previous_id) {
            chain_anchor = AnchorContext {
                anchor: PreviewAnchor::AfterBlock { block_id },
                resolved: chain_anchor.resolved,
            };
        }
    }
}

fn push_preview_block(
    block: PreviewBlock,
    blocks: &mut Vec<PreviewBlock>,
    seen: &mut BTreeSet<String>,
) {
    let key = block
        .block_id
        .clone()
        .unwrap_or_else(|| format!("{}:{}", block.block_type, blocks.len()));
    if seen.insert(key) {
        blocks.push(block);
    }
}

fn collect_leaf_columns<'a>(
    cols: &'a [Value],
    column_keys: &mut Vec<&'a str>,
    header_labels: &mut Vec<String>,
) {
    for col in cols {
        let Some(col_obj) = col.as_object() else {
            continue;
        };
        if let Some(children) = col_obj.get("children").and_then(Value::as_array)
            && !children.is_empty()
        {
            collect_leaf_columns(children, column_keys, header_labels);
            continue;
        }
        if let Some(key) = col_obj.get("key").and_then(Value::as_str) {
            column_keys.push(key);
        }
        let label = col_obj
            .get("label")
            .or_else(|| col_obj.get("title"))
            .or_else(|| col_obj.get("key"))
            .and_then(Value::as_str)
            .map(str::trim)
            .filter(|value| !value.is_empty());
        if let Some(label) = label {
            header_labels.push(label.to_string());
        }
    }
}

/// Synthesize a pipe-separated table preview (`Col A | Col B\nRow1A | Row1B`)
/// from partial `columns` (including nested `ColumnNode` trees) and `rows` while a `table` block is streaming.
fn synthesize_table_preview_text(object: &serde_json::Map<String, Value>) -> Option<String> {
    let columns = object.get("columns").and_then(Value::as_array);
    let rows = object.get("rows").and_then(Value::as_array);
    let mut column_keys: Vec<&str> = Vec::new();
    let mut header_labels: Vec<String> = Vec::new();
    if let Some(cols) = columns {
        collect_leaf_columns(cols, &mut column_keys, &mut header_labels);
    }
    let mut lines: Vec<String> = Vec::new();
    if !header_labels.is_empty() {
        lines.push(header_labels.join(" | "));
    }
    if let Some(row_list) = rows {
        for row in row_list {
            let cells: Vec<String> = match row {
                Value::Object(row_obj) => {
                    if !column_keys.is_empty() {
                        column_keys
                            .iter()
                            .map(|key| format_cell_preview(row_obj.get(*key)))
                            .collect()
                    } else {
                        row_obj
                            .values()
                            .map(|val| format_cell_preview(Some(val)))
                            .collect()
                    }
                }
                Value::Array(arr) => arr
                    .iter()
                    .map(|val| format_cell_preview(Some(val)))
                    .collect(),
                _ => Vec::new(),
            };
            if !cells.is_empty() && cells.iter().any(|cell| !cell.is_empty()) {
                lines.push(cells.join(" | "));
            }
        }
    }
    if lines.is_empty() {
        object
            .get("note")
            .and_then(Value::as_str)
            .map(str::to_string)
    } else {
        Some(lines.join("\n"))
    }
}

fn format_cell_preview(value: Option<&Value>) -> String {
    match value {
        Some(Value::String(text)) => text.trim().replace('\n', " "),
        Some(Value::Number(num)) => num.to_string(),
        Some(Value::Bool(flag)) => flag.to_string(),
        Some(Value::Object(obj)) => obj
            .get("text")
            .or_else(|| obj.get("value"))
            .and_then(Value::as_str)
            .map(|text| text.trim().replace('\n', " "))
            .or_else(|| {
                obj.get("number")
                    .or_else(|| obj.get("value"))
                    .and_then(Value::as_f64)
                    .map(|n| n.to_string())
            })
            .unwrap_or_default(),
        _ => String::new(),
    }
}

fn preview_block(
    object: &serde_json::Map<String, Value>,
    anchor: &AnchorContext,
) -> Option<PreviewBlock> {
    let inferred_type = if object.contains_key("level") {
        "heading"
    } else if object.contains_key("rows")
        || object.contains_key("columns")
        || object.contains_key("caption")
    {
        "table"
    } else if object.contains_key("option")
        || object.contains_key("option_patch")
        || object.contains_key("fallback_summary")
    {
        "echarts"
    } else if object.contains_key("html") || object.contains_key("fallback_text") {
        "custom_html"
    } else if object.contains_key("attribution") {
        "quote"
    } else {
        "paragraph"
    };
    let block_type = object
        .get("type")
        .and_then(Value::as_str)
        .unwrap_or(inferred_type);
    if !matches!(
        block_type,
        "heading" | "paragraph" | "quote" | "table" | "echarts" | "custom_html"
    ) {
        return None;
    }

    let title = object
        .get("title")
        .or_else(|| object.get("caption"))
        .or_else(|| {
            (block_type == "quote")
                .then(|| object.get("attribution"))
                .flatten()
        })
        .and_then(Value::as_str)
        .map(str::to_string)
        .or_else(|| {
            (block_type == "echarts")
                .then(|| {
                    object
                        .get("option")
                        .and_then(Value::as_object)
                        .and_then(|opt| opt.get("title"))
                        .and_then(Value::as_object)
                        .and_then(|t| t.get("text"))
                        .and_then(Value::as_str)
                        .map(str::to_string)
                })
                .flatten()
        });

    let text = object
        .get("text")
        .and_then(Value::as_str)
        .map(str::to_string)
        .or_else(|| match block_type {
            "table" => synthesize_table_preview_text(object),
            "echarts" => object
                .get("fallback_summary")
                .or_else(|| object.get("note"))
                .and_then(Value::as_str)
                .map(str::to_string),
            "custom_html" => object
                .get("fallback_text")
                .or_else(|| object.get("html"))
                .and_then(Value::as_str)
                .map(str::to_string),
            _ => None,
        });

    if text.is_none() && title.is_none() {
        return None;
    }

    let block_id = object
        .get("id")
        .and_then(Value::as_str)
        .filter(|value| BlockId::parse((*value).to_string()).is_ok())
        .map(str::to_string);
    let level = object
        .get("level")
        .and_then(Value::as_u64)
        .and_then(|value| u8::try_from(value).ok());
    Some(PreviewBlock {
        block_id,
        block_type: block_type.to_string(),
        title,
        level,
        text,
        anchor: anchor.anchor.clone(),
        anchor_resolved: anchor.resolved,
    })
}
