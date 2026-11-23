use super::*;

pub(crate) fn apply_block_patch(block: &mut Block, patch: &BlockPatch) -> Result<(), PatchError> {
    validate_patch_compatibility(block, patch)?;
    let block_id = block.id.clone();

    if let Some(style_patch) = &patch.style {
        apply_style_patch(&mut block.style, style_patch);
    }
    if let Some(class_name) = &patch.class_name {
        block.class_name = if class_name.trim().is_empty() {
            None
        } else {
            Some(class_name.clone())
        };
    }
    if let Some(source_ids) = &patch.source_ids {
        block.source_ids = source_ids.clone();
    }
    if let Some(note) = &patch.source_note {
        block.source_note = if note.trim().is_empty() {
            None
        } else {
            Some(note.clone())
        };
    }
    if let Some(lint_ignore) = &patch.lint_ignore {
        block.lint_ignore = lint_ignore
            .iter()
            .map(|entry| entry.trim())
            .filter(|entry| !entry.is_empty())
            .map(ToString::to_string)
            .collect();
    }

    match &mut block.body {
        BlockKind::Heading { level, text } => {
            if let Some(new_text) = &patch.text {
                *text = new_text.clone();
            }
            if let Some(new_level) = patch.level {
                if !(1..=6).contains(&new_level) {
                    return Err(PatchError::InvalidPatch {
                        block_id,
                        message: format!("heading level must be 1..=6, received {new_level}"),
                    });
                }
                *level = new_level;
            }
        }
        BlockKind::Paragraph { text } | BlockKind::Quote { text } => {
            if let Some(new_text) = &patch.text {
                *text = new_text.clone();
            }
        }
        BlockKind::Table(table) => {
            if let Some(title) = &patch.title {
                table.title = (!title.trim().is_empty()).then(|| title.clone());
            }
            if let Some(numbering) = patch.numbering {
                table.numbering = Some(numbering);
            }
            if let Some(note) = &patch.note {
                table.note = (!note.trim().is_empty()).then(|| note.clone());
            }
            if let Some(columns) = &patch.columns {
                table.columns = columns.clone();
                validate_table(table).map_err(|error| PatchError::InvalidPatch {
                    block_id: block_id.clone(),
                    message: error.to_string(),
                })?;
            }
            if let Some(rows) = &patch.rows {
                table.rows = rows.clone();
            }
            if let Some(summary_rows) = &patch.summary_rows {
                table.summary_rows = summary_rows.clone();
            }
            if !patch.append_rows.is_empty() {
                table.rows.extend(patch.append_rows.iter().cloned());
            }
            if let Some(cell_update) = &patch.update_cell {
                apply_cell_update(block_id, table, cell_update)?;
            }
        }
        BlockKind::Echarts(chart) => {
            if let Some(title) = &patch.title {
                chart.title = (!title.trim().is_empty()).then(|| title.clone());
            }
            if let Some(numbering) = patch.numbering {
                chart.numbering = Some(numbering);
            }
            if let Some(note) = &patch.note {
                chart.note = (!note.trim().is_empty()).then(|| note.clone());
            }
            if let Some(option) = &patch.option {
                chart.option = option.clone();
                chart.option_patch = None;
            }
            if let Some(height) = patch.height {
                chart.height = height;
            }
            if let Some(option_patch) = &patch.option_patch {
                chart.option_patch = Some(merge_chart_option_patch(
                    chart.option_patch.as_ref(),
                    option_patch,
                ));
            }
            if let Some(summary) = &patch.fallback_summary {
                chart.fallback_summary = summary.clone();
            }
        }
        BlockKind::CustomHtml(html) => {
            if let Some(title) = &patch.title {
                html.title = (!title.trim().is_empty()).then(|| title.clone());
            }
            if let Some(numbering) = patch.numbering {
                html.numbering = Some(numbering);
            }
            if let Some(note) = &patch.note {
                html.note = (!note.trim().is_empty()).then(|| note.clone());
            }
            if let Some(text) = &patch.fallback_text {
                html.fallback_text = text.clone();
            }
        }
    }
    Ok(())
}

fn validate_patch_compatibility(block: &Block, patch: &BlockPatch) -> Result<(), PatchError> {
    let (supported, supported_text) = match &block.body {
        BlockKind::Heading { .. } => (&["text", "level"][..], "text, level"),
        BlockKind::Paragraph { .. } | BlockKind::Quote { .. } => (&["text"][..], "text"),
        BlockKind::Table(_) => (
            &[
                "title",
                "numbering",
                "note",
                "columns",
                "rows",
                "summary_rows",
                "append_rows",
                "update_cell",
            ][..],
            "title, numbering, note, columns, rows, summary_rows, append_rows, update_cell",
        ),
        BlockKind::Echarts(_) => (
            &[
                "title",
                "numbering",
                "note",
                "option",
                "option_patch",
                "height",
                "fallback_summary",
            ][..],
            "title, numbering, note, option, option_patch, height, fallback_summary",
        ),
        BlockKind::CustomHtml(_) => (
            &["title", "numbering", "note", "fallback_text"][..],
            "title, numbering, note, fallback_text",
        ),
    };
    let present = [
        ("text", patch.text.is_some()),
        ("level", patch.level.is_some()),
        ("title", patch.title.is_some()),
        ("numbering", patch.numbering.is_some()),
        ("note", patch.note.is_some()),
        ("fallback_text", patch.fallback_text.is_some()),
        ("fallback_summary", patch.fallback_summary.is_some()),
        ("option", patch.option.is_some()),
        ("option_patch", patch.option_patch.is_some()),
        ("height", patch.height.is_some()),
        ("columns", patch.columns.is_some()),
        ("rows", patch.rows.is_some()),
        ("summary_rows", patch.summary_rows.is_some()),
        ("append_rows", !patch.append_rows.is_empty()),
        ("update_cell", patch.update_cell.is_some()),
    ];
    for (field, value) in [
        ("option", patch.option.as_ref()),
        ("option_patch", patch.option_patch.as_ref()),
    ] {
        if value.is_some_and(|value| !value.is_object()) {
            return Err(PatchError::InvalidPatch {
                block_id: block.id.clone(),
                message: format!("{field} must be a JSON object"),
            });
        }
    }
    let unsupported: Vec<&str> = present
        .iter()
        .filter(|(field, is_present)| *is_present && !supported.contains(field))
        .map(|(field, _)| *field)
        .collect();
    if unsupported.is_empty() {
        return Ok(());
    }
    Err(PatchError::UnsupportedPatch {
        block_id: block.id.clone(),
        block_type: block.block_type(),
        fields: unsupported.join(", "),
        supported: supported_text.to_string(),
    })
}

fn apply_style_patch(style: &mut BlockStyle, patch: &BlockStylePatch) {
    if let Some(variant) = &patch.variant {
        style.variant = if variant.trim().is_empty() {
            None
        } else {
            Some(variant.clone())
        };
    }
    if let Some(keep_together) = patch.keep_together {
        style.keep_together = keep_together;
    }
    if let Some(page_break_before) = patch.page_break_before {
        style.page_break_before = page_break_before;
    }
    if let Some(column_widths) = &patch.column_widths {
        style.column_widths = Some(column_widths.clone());
    }
}

fn apply_cell_update(
    block_id: BlockId,
    table: &mut crate::block::TableBlock,
    update: &CellUpdate,
) -> Result<(), PatchError> {
    let shape = validate_table(table).map_err(|error| PatchError::InvalidCellUpdate {
        block_id: block_id.clone(),
        message: error.to_string(),
    })?;
    if !shape.has_key(&update.column_key) {
        return Err(PatchError::InvalidCellUpdate {
            block_id,
            message: format!(
                "unknown column_key `{}`; valid leaf keys: {}",
                update.column_key,
                shape
                    .leaves
                    .iter()
                    .map(|leaf| leaf.key.as_str())
                    .collect::<Vec<_>>()
                    .join(", ")
            ),
        });
    }
    let rows = if update.summary {
        &mut table.summary_rows
    } else {
        &mut table.rows
    };
    let Some(row) = rows.get_mut(update.row_index) else {
        return Err(PatchError::InvalidCellUpdate {
            block_id,
            message: format!(
                "row_index {} is out of range for {} (valid: 0..={})",
                update.row_index,
                if update.summary {
                    "summary_rows"
                } else {
                    "rows"
                },
                rows.len().saturating_sub(1)
            ),
        });
    };
    row.insert(update.column_key.clone(), update.value.clone());
    Ok(())
}
