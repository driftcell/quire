use super::*;

pub fn remap_source_references(block: &mut Block, mapping: &BTreeMap<SourceId, SourceId>) {
    if mapping.is_empty() {
        return;
    }
    let mut seen = BTreeSet::new();
    block.source_ids = block
        .source_ids
        .iter()
        .map(|source_id| {
            mapping
                .get(source_id)
                .cloned()
                .unwrap_or_else(|| source_id.clone())
        })
        .filter(|source_id| seen.insert(source_id.clone()))
        .collect();
    let rewrite = |text: &mut String| {
        for (from, to) in mapping {
            if from != to {
                *text = text.replace(&format!("[^{from}]"), &format!("[^{to}]"));
            }
        }
    };
    match &mut block.body {
        BlockKind::Heading { text, .. }
        | BlockKind::Paragraph { text }
        | BlockKind::Quote { text } => rewrite(text),
        BlockKind::Table(table) => {
            if let Some(title) = &mut table.title {
                rewrite(title);
            }
        }
        BlockKind::Echarts(_) | BlockKind::CustomHtml(_) => {}
    }
}

/// Rewrite source references across a patch batch.
pub fn remap_operations_sources(ops: &mut [PatchOp], mapping: &BTreeMap<SourceId, SourceId>) {
    if mapping.is_empty() {
        return;
    }
    for op in ops {
        match op {
            PatchOp::InsertAfter { blocks, .. }
            | PatchOp::InsertBefore { blocks, .. }
            | PatchOp::Append { blocks }
            | PatchOp::AppendToSection { blocks, .. }
            | PatchOp::ReplaceRange { blocks, .. } => {
                for block in blocks {
                    remap_source_references(block, mapping);
                }
            }
            PatchOp::PatchBlock { patch, .. } => {
                if let Some(source_ids) = &mut patch.source_ids {
                    *source_ids = source_ids
                        .iter()
                        .map(|source_id| {
                            mapping
                                .get(source_id)
                                .cloned()
                                .unwrap_or_else(|| source_id.clone())
                        })
                        .collect();
                }
                if let Some(text) = &mut patch.text {
                    for (from, to) in mapping {
                        if from != to {
                            *text = text.replace(&format!("[^{from}]"), &format!("[^{to}]"));
                        }
                    }
                }
            }
            PatchOp::SetSources { sources } => {
                let mut remapped = IndexMap::new();
                for (source_id, source) in std::mem::take(sources) {
                    remapped.insert(
                        mapping.get(&source_id).cloned().unwrap_or(source_id),
                        source,
                    );
                }
                *sources = remapped;
            }
            PatchOp::DeleteBlocks { .. }
            | PatchOp::MoveBlock { .. }
            | PatchOp::SetCover { .. }
            | PatchOp::SetToc { .. }
            | PatchOp::SetReferences { .. }
            | PatchOp::SetLint { .. }
            | PatchOp::SetTheme { .. } => {}
        }
    }
}
