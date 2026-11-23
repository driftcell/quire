use super::*;

pub fn apply_operations(
    document: &ReportDocument,
    ops: &[PatchOp],
) -> Result<(ReportDocument, PatchOutcome), PatchError> {
    let mut next = document.clone();
    let outcome = apply_operations_in_place(&mut next, ops)?;
    Ok((next, outcome))
}

/// Apply operations in place. The input is cloned internally so a failed batch never leaves the
/// caller's document partially mutated.
pub fn apply_operations_in_place(
    document: &mut ReportDocument,
    ops: &[PatchOp],
) -> Result<PatchOutcome, PatchError> {
    let mut working = document.clone();
    let outcome = apply_internal(&mut working, ops)?;
    *document = working;
    Ok(outcome)
}

fn apply_internal(
    document: &mut ReportDocument,
    ops: &[PatchOp],
) -> Result<PatchOutcome, PatchError> {
    let mut outcome = PatchOutcome::default();
    let mut touched = BTreeSet::new();
    let mut applied_ops = Vec::with_capacity(ops.len());
    for op in ops {
        match op {
            PatchOp::InsertAfter { target_id, blocks } => {
                let index = require_position(document, target_id)? + 1;
                let blocks = forgiving_blocks(document, blocks, &mut outcome);
                validate_new_blocks(document, &blocks)?;
                insert_blocks(document, index, &blocks, &mut outcome, &mut touched);
                applied_ops.push(PatchOp::InsertAfter {
                    target_id: target_id.clone(),
                    blocks,
                });
            }
            PatchOp::InsertBefore { target_id, blocks } => {
                let index = require_position(document, target_id)?;
                let blocks = forgiving_blocks(document, blocks, &mut outcome);
                validate_new_blocks(document, &blocks)?;
                insert_blocks(document, index, &blocks, &mut outcome, &mut touched);
                applied_ops.push(PatchOp::InsertBefore {
                    target_id: target_id.clone(),
                    blocks,
                });
            }
            PatchOp::Append { blocks } => {
                let blocks = forgiving_blocks(document, blocks, &mut outcome);
                validate_new_blocks(document, &blocks)?;
                let index = document.blocks.len();
                insert_blocks(document, index, &blocks, &mut outcome, &mut touched);
                applied_ops.push(PatchOp::Append { blocks });
            }
            PatchOp::AppendToSection { section_id, blocks } => {
                let section_index = require_position(document, section_id)?;
                let level = match document.blocks.get(section_index).map(|block| &block.body) {
                    Some(BlockKind::Heading { level, .. }) => *level,
                    _ => {
                        return Err(PatchError::InvalidPatch {
                            block_id: section_id.clone(),
                            message: "AppendToSection requires a heading section_id".into(),
                        });
                    }
                };
                let mut index = section_index + 1;
                while let Some(block) = document.blocks.get(index) {
                    if let BlockKind::Heading { level: next, .. } = &block.body
                        && *next <= level
                    {
                        break;
                    }
                    index += 1;
                }
                let blocks = forgiving_blocks(document, blocks, &mut outcome);
                validate_new_blocks(document, &blocks)?;
                insert_blocks(document, index, &blocks, &mut outcome, &mut touched);
                touched.insert(section_id.clone());
                applied_ops.push(PatchOp::AppendToSection {
                    section_id: section_id.clone(),
                    blocks,
                });
            }
            PatchOp::PatchBlock { block_id, patch } => {
                if patch.is_empty() {
                    return Err(PatchError::EmptyPatch {
                        block_id: block_id.clone(),
                    });
                }
                let index = require_position(document, block_id)?;
                let valid_ids = valid_block_ids(document);
                let block =
                    document
                        .blocks
                        .get_mut(index)
                        .ok_or_else(|| PatchError::BlockNotFound {
                            block_id: block_id.clone(),
                            valid_ids: valid_ids.clone(),
                        })?;
                apply_block_patch(block, patch)?;
                outcome.updated_block_ids.push(block_id.clone());
                touched.insert(block_id.clone());
                applied_ops.push(op.clone());
            }
            PatchOp::ReplaceRange {
                start_id,
                end_id,
                blocks,
            } => {
                let start = require_position(document, start_id)?;
                let end = require_position(document, end_id)?;
                if start > end {
                    return Err(PatchError::InvalidRange {
                        start_id: start_id.clone(),
                        end_id: end_id.clone(),
                    });
                }
                let Some(range) = document.blocks.get(start..=end) else {
                    return Err(PatchError::InvalidRange {
                        start_id: start_id.clone(),
                        end_id: end_id.clone(),
                    });
                };
                let removed: Vec<BlockId> = range.iter().map(|block| block.id.clone()).collect();
                for id in &removed {
                    touched.insert(id.clone());
                    outcome.deleted_block_ids.push(id.clone());
                }
                document.blocks.drain(start..=end);
                let blocks = forgiving_blocks(document, blocks, &mut outcome);
                validate_new_blocks(document, &blocks)?;
                insert_blocks(document, start, &blocks, &mut outcome, &mut touched);
                applied_ops.push(PatchOp::ReplaceRange {
                    start_id: start_id.clone(),
                    end_id: end_id.clone(),
                    blocks,
                });
            }
            PatchOp::DeleteBlocks { block_ids } => {
                let mut unique: Vec<BlockId> = Vec::new();
                let mut seen = BTreeSet::new();
                for block_id in block_ids {
                    if seen.insert(block_id.clone()) {
                        unique.push(block_id.clone());
                    }
                }
                for block_id in &unique {
                    require_position(document, block_id)?;
                }
                let to_delete: BTreeSet<&str> =
                    unique.iter().map(|block_id| block_id.as_str()).collect();
                document
                    .blocks
                    .retain(|block| !to_delete.contains(block.id.as_str()));
                for block_id in unique {
                    touched.insert(block_id.clone());
                    outcome.deleted_block_ids.push(block_id);
                }
                applied_ops.push(op.clone());
            }
            PatchOp::MoveBlock { block_id, after_id } => {
                let from = require_position(document, block_id)?;
                if after_id.as_ref() == Some(block_id) {
                    return Err(PatchError::MoveIntoSelf {
                        block_id: block_id.clone(),
                    });
                }
                if let Some(after_id) = after_id {
                    require_position(document, after_id)?;
                }
                let block = document.blocks.remove(from);
                let insert_at = match after_id {
                    None => 0,
                    Some(after_id) => require_position(document, after_id)
                        .map(|index| index + 1)
                        .unwrap_or(0),
                };
                document.blocks.insert(insert_at, block);
                touched.insert(block_id.clone());
                if let Some(after_id) = after_id {
                    touched.insert(after_id.clone());
                }
                outcome.moved_block_ids.push(block_id.clone());
                applied_ops.push(op.clone());
            }
            PatchOp::SetCover { cover } => {
                document.cover = cover.as_ref().clone();
                outcome.metadata_changed = true;
                applied_ops.push(op.clone());
            }
            PatchOp::SetToc { toc } => {
                document.toc = toc.clone();
                outcome.metadata_changed = true;
                applied_ops.push(op.clone());
            }
            PatchOp::SetReferences { references } => {
                document.references = references.clone();
                outcome.metadata_changed = true;
                applied_ops.push(op.clone());
            }
            PatchOp::SetLint { lint } => {
                document.lint = lint.clone();
                outcome.metadata_changed = true;
                applied_ops.push(op.clone());
            }
            PatchOp::SetTheme { theme } => {
                document.theme = theme.as_ref().clone();
                outcome.metadata_changed = true;
                applied_ops.push(op.clone());
            }
            PatchOp::SetSources { sources } => {
                document.sources = sources.clone();
                outcome.metadata_changed = true;
                applied_ops.push(op.clone());
            }
        }
    }
    outcome.affected_block_ids = touched.into_iter().collect();
    outcome.applied_ops = applied_ops;
    outcome.no_op = outcome.inserted_block_ids.is_empty()
        && outcome.updated_block_ids.is_empty()
        && outcome.deleted_block_ids.is_empty()
        && outcome.moved_block_ids.is_empty()
        && !outcome.metadata_changed;
    Ok(outcome)
}

/// Rewrite source references inside a block using a canonical-ID mapping.
fn forgiving_blocks(
    document: &ReportDocument,
    blocks: &[Block],
    outcome: &mut PatchOutcome,
) -> Vec<Block> {
    let mut used: BTreeSet<BlockId> = document
        .blocks
        .iter()
        .map(|block| block.id.clone())
        .collect();
    let mut output = Vec::with_capacity(blocks.len());
    for block in blocks {
        let mut block = block.clone();
        let requested = if block.id.is_pending() {
            None
        } else {
            Some(block.id.clone())
        };
        if block.id.is_pending() || used.contains(&block.id) {
            let prefix = block.block_type();
            let mut sequence = 1usize;
            let assigned = loop {
                if let Ok(candidate) = BlockId::parse(format!("b_{prefix}_{sequence}"))
                    && !used.contains(&candidate)
                {
                    break candidate;
                }
                sequence += 1;
                if sequence > 100_000 {
                    break block.id.clone();
                }
            };
            block.id = assigned.clone();
            outcome.assigned_ids.push(IdAssignment {
                requested,
                assigned,
            });
        }
        used.insert(block.id.clone());
        output.push(block);
    }
    output
}

fn insert_blocks(
    document: &mut ReportDocument,
    index: usize,
    blocks: &[Block],
    outcome: &mut PatchOutcome,
    touched: &mut BTreeSet<BlockId>,
) {
    for (offset, block) in blocks.iter().enumerate() {
        outcome
            .id_mapping
            .insert(block.id.clone(), block.id.clone());
        outcome.inserted_block_ids.push(block.id.clone());
        touched.insert(block.id.clone());
        document.blocks.insert(index + offset, block.clone());
    }
}

fn require_position(document: &ReportDocument, block_id: &BlockId) -> Result<usize, PatchError> {
    document
        .position(block_id.as_str())
        .ok_or_else(|| PatchError::BlockNotFound {
            block_id: block_id.clone(),
            valid_ids: valid_block_ids(document),
        })
}

fn valid_block_ids(document: &ReportDocument) -> String {
    if document.blocks.is_empty() {
        "(none; append or insert the first block)".to_string()
    } else {
        document
            .blocks
            .iter()
            .map(|block| block.id.as_str())
            .collect::<Vec<_>>()
            .join(", ")
    }
}

fn validate_new_blocks(document: &ReportDocument, blocks: &[Block]) -> Result<(), PatchError> {
    let existing: BTreeSet<&str> = document
        .blocks
        .iter()
        .map(|block| block.id.as_str())
        .collect();
    let mut batch: BTreeSet<&str> = BTreeSet::new();
    for block in blocks {
        if !is_semantic_block_id(block.id.as_str()) {
            return Err(PatchError::InvalidBlockId {
                block_id: block.id.clone(),
                reason: "use 1-64 ASCII letters/digits plus '-' '_' or '.', starting alphanumeric; never a UUIDv4".to_string(),
            });
        }
        if existing.contains(block.id.as_str()) || !batch.insert(block.id.as_str()) {
            return Err(PatchError::DuplicateBlockId {
                block_id: block.id.clone(),
                valid_ids: valid_block_ids(document),
            });
        }
    }
    Ok(())
}
