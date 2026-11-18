//! Declarative block patch operations.
//!
//! Operations are intentionally frontend-agnostic. The store applies them transactionally,
//! computes inverse operations, and broadcasts the same JSON to read-only SSE clients.

use std::collections::{BTreeMap, BTreeSet};

use indexmap::IndexMap;
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use thiserror::Error;

use crate::{
    block::{Block, BlockKind, BlockStyle, CellValue, NumberingKind, is_semantic_block_id},
    ids::{BlockId, SourceId},
    json::merge_optional_json,
    model::{CoverConfig, ReferencesConfig, ReportDocument, SourceEntry, ThemeConfig, TocConfig},
    table::validate_table,
};

/// Partial style update. Omitted fields are preserved; empty string variants clear the field.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct BlockStylePatch {
    /// Renderer hint. Empty string clears it.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub variant: Option<String>,
    /// Prevent page splitting where supported.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub keep_together: Option<bool>,
    /// Force a page break before this block.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub page_break_before: Option<bool>,
    /// Per-leaf-column CSS widths keyed by column key. Example: `{"region":"24%","q1":"120px"}`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub column_widths: Option<BTreeMap<String, String>>,
}

impl BlockStylePatch {
    pub fn is_empty(&self) -> bool {
        self.variant.is_none()
            && self.keep_together.is_none()
            && self.page_break_before.is_none()
            && self.column_widths.is_none()
    }
}

/// Update one table cell by row index and leaf key.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct CellUpdate {
    /// Zero-based row index within `rows` (or `summary_rows` when `summary = true`).
    pub row_index: usize,
    /// Leaf column key whose cell should be replaced.
    pub column_key: String,
    /// New primitive or rich cell value.
    pub value: CellValue,
    /// Update `summary_rows` instead of body `rows`.
    #[serde(default, skip_serializing_if = "is_false")]
    pub summary: bool,
}

const fn is_false(value: &bool) -> bool {
    !*value
}

/// Partial patch for a single block.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct BlockPatch {
    /// Heading, paragraph or quote text.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub text: Option<String>,
    /// Heading level (1..=6).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub level: Option<u8>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub class_name: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub source_ids: Option<Vec<SourceId>>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub source_note: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub style: Option<BlockStylePatch>,
    /// Deep-merged into an ECharts block's existing `option_patch`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub option_patch: Option<serde_json::Value>,
    /// Replace the ECharts fallback summary.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub fallback_summary: Option<String>,
    /// Replace the outer visual title. Empty string clears it. `caption` is accepted as a legacy
    /// alias.
    #[serde(default, alias = "caption", skip_serializing_if = "Option::is_none")]
    pub title: Option<String>,
    /// Replace automatic figure/table numbering; `null` leaves it unchanged and `"none"` disables.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub numbering: Option<NumberingKind>,
    /// Replace the explanatory footnote. Empty string clears it.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub note: Option<String>,
    /// Replace custom HTML fallback text.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub fallback_text: Option<String>,
    /// Append rows to a table's body.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub append_rows: Vec<BTreeMap<String, CellValue>>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub update_cell: Option<CellUpdate>,
}

impl BlockPatch {
    pub fn is_empty(&self) -> bool {
        self.text.is_none()
            && self.level.is_none()
            && self.class_name.is_none()
            && self.source_ids.is_none()
            && self.source_note.is_none()
            && self.style.as_ref().is_none_or(BlockStylePatch::is_empty)
            && self.option_patch.is_none()
            && self.fallback_summary.is_none()
            && self.title.is_none()
            && self.numbering.is_none()
            && self.note.is_none()
            && self.fallback_text.is_none()
            && self.append_rows.is_empty()
            && self.update_cell.is_none()
    }
}

/// A declarative mutation. Deserialized with a `type` discriminator.
///
/// The large `PatchBlock` variant is intentional: boxing it would force every JSON caller and
/// store adapter to unwrap an extra pointer for a cold error path.
#[allow(clippy::large_enum_variant)]
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum PatchOp {
    /// Insert blocks immediately after an existing anchor block.
    InsertAfter {
        /// Existing block ID used as the anchor.
        target_id: BlockId,
        /// New blocks in display order.
        blocks: Vec<Block>,
    },
    /// Insert blocks immediately before an existing anchor block.
    InsertBefore {
        /// Existing block ID used as the anchor.
        target_id: BlockId,
        /// New blocks in display order.
        blocks: Vec<Block>,
    },
    /// Append blocks at the end of the report.
    Append {
        /// New blocks in display order.
        blocks: Vec<Block>,
    },
    /// Insert blocks at the end of a heading section (before the next heading of the same or
    /// higher level, or at end-of-document).
    AppendToSection {
        /// Heading block that owns the section.
        section_id: BlockId,
        /// New blocks in display order.
        blocks: Vec<Block>,
    },
    /// Partially update one existing block.
    PatchBlock {
        /// Target block ID.
        block_id: BlockId,
        /// Partial patch. Unsupported fields return a self-healing error.
        patch: BlockPatch,
    },
    /// Replace an inclusive range with new blocks.
    ReplaceRange {
        /// First block ID in the replaced range.
        start_id: BlockId,
        /// Last block ID in the replaced range (inclusive).
        end_id: BlockId,
        /// Replacement blocks. Empty means delete the range.
        blocks: Vec<Block>,
    },
    /// Delete one or more blocks.
    DeleteBlocks {
        /// Block IDs to delete. All must exist.
        block_ids: Vec<BlockId>,
    },
    /// Move one block. `after_id: null` moves it to the top.
    MoveBlock {
        /// Block ID to move.
        block_id: BlockId,
        /// Anchor block; null means the beginning of the report.
        after_id: Option<BlockId>,
    },
    /// Server-side configuration operation. Dedicated MCP tools use this internally so that
    /// configuration changes are revisioned, streamed and reversible just like block changes.
    SetCover {
        /// Complete replacement cover config.
        cover: CoverConfig,
    },
    SetToc {
        /// Complete replacement TOC config.
        toc: TocConfig,
    },
    SetReferences {
        /// Complete replacement bibliography config.
        references: ReferencesConfig,
    },
    SetTheme {
        /// Complete replacement theme config.
        theme: ThemeConfig,
    },
    /// Replace the full source registry. Used for rollback and source deduplication.
    SetSources {
        /// Complete replacement source registry keyed by source_id.
        #[schemars(with = "std::collections::BTreeMap<SourceId, SourceEntry>")]
        sources: IndexMap<SourceId, SourceEntry>,
    },
}

impl PatchOp {
    /// All block IDs read or written by this operation. Used by Smart CAS to decide whether an
    /// intervening revision conflicts with an incoming batch.
    pub fn touched_block_ids(&self) -> BTreeSet<BlockId> {
        let mut ids = BTreeSet::new();
        match self {
            Self::InsertAfter { target_id, blocks } | Self::InsertBefore { target_id, blocks } => {
                ids.insert(target_id.clone());
                ids.extend(blocks.iter().map(|block| block.id.clone()));
            }
            Self::Append { blocks } | Self::AppendToSection { blocks, .. } => {
                ids.extend(blocks.iter().map(|block| block.id.clone()));
                if let Self::AppendToSection { section_id, .. } = self {
                    ids.insert(section_id.clone());
                }
            }
            Self::PatchBlock { block_id, .. } => {
                ids.insert(block_id.clone());
            }
            Self::ReplaceRange {
                start_id,
                end_id,
                blocks,
            } => {
                ids.insert(start_id.clone());
                ids.insert(end_id.clone());
                ids.extend(blocks.iter().map(|block| block.id.clone()));
            }
            Self::DeleteBlocks { block_ids } => {
                ids.extend(block_ids.iter().cloned());
            }
            Self::MoveBlock { block_id, after_id } => {
                ids.insert(block_id.clone());
                if let Some(after_id) = after_id {
                    ids.insert(after_id.clone());
                }
            }
            Self::SetCover { .. }
            | Self::SetToc { .. }
            | Self::SetReferences { .. }
            | Self::SetTheme { .. }
            | Self::SetSources { .. } => {}
        }
        ids
    }

    /// IDs of blocks introduced by this operation, before collision handling.
    pub fn introduced_block_ids(&self) -> Vec<BlockId> {
        match self {
            Self::InsertAfter { blocks, .. }
            | Self::InsertBefore { blocks, .. }
            | Self::Append { blocks }
            | Self::AppendToSection { blocks, .. }
            | Self::ReplaceRange { blocks, .. } => {
                blocks.iter().map(|block| block.id.clone()).collect()
            }
            Self::PatchBlock { .. }
            | Self::DeleteBlocks { .. }
            | Self::MoveBlock { .. }
            | Self::SetCover { .. }
            | Self::SetToc { .. }
            | Self::SetReferences { .. }
            | Self::SetTheme { .. }
            | Self::SetSources { .. } => Vec::new(),
        }
    }

    pub const fn is_metadata(&self) -> bool {
        matches!(
            self,
            Self::SetCover { .. }
                | Self::SetToc { .. }
                | Self::SetReferences { .. }
                | Self::SetTheme { .. }
                | Self::SetSources { .. }
        )
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Error)]
pub enum PatchError {
    #[error("block `{block_id}` was not found. Valid block_ids: {valid_ids}")]
    BlockNotFound {
        block_id: BlockId,
        valid_ids: String,
    },
    #[error("new block id `{block_id}` is invalid: {reason}")]
    InvalidBlockId { block_id: BlockId, reason: String },
    #[error("new block id `{block_id}` already exists. Existing block_ids: {valid_ids}")]
    DuplicateBlockId {
        block_id: BlockId,
        valid_ids: String,
    },
    #[error("operation range is invalid: start `{start_id}` appears after end `{end_id}`")]
    InvalidRange { start_id: BlockId, end_id: BlockId },
    #[error("cannot move block `{block_id}` after itself")]
    MoveIntoSelf { block_id: BlockId },
    #[error(
        "block `{block_id}` ({block_type}) does not support patch fields: {fields}. Supported: {supported}"
    )]
    UnsupportedPatch {
        block_id: BlockId,
        block_type: &'static str,
        fields: String,
        supported: String,
    },
    #[error("block `{block_id}` patch is empty; supply at least one field")]
    EmptyPatch { block_id: BlockId },
    #[error("invalid patch for block `{block_id}`: {message}")]
    InvalidPatch { block_id: BlockId, message: String },
    #[error("table cell update failed for block `{block_id}`: {message}")]
    InvalidCellUpdate { block_id: BlockId, message: String },
}

/// Mapping produced when an empty or colliding block ID was auto-assigned.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
pub struct IdAssignment {
    /// Original ID, or `None` when the caller omitted/emptied it.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub requested: Option<BlockId>,
    /// Final unique semantic ID.
    pub assigned: BlockId,
}

/// Receipt for an applied mutation batch.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct PatchOutcome {
    pub inserted_block_ids: Vec<BlockId>,
    pub updated_block_ids: Vec<BlockId>,
    pub deleted_block_ids: Vec<BlockId>,
    pub moved_block_ids: Vec<BlockId>,
    pub affected_block_ids: Vec<BlockId>,
    /// Map of submitted block IDs to final IDs. `apply_operations` is identity; `apply_draft`
    /// may prefix collisions.
    pub id_mapping: BTreeMap<BlockId, BlockId>,
    pub metadata_changed: bool,
    pub no_op: bool,
    /// Block IDs auto-assigned because the submitted ID was empty or colliding.
    pub assigned_ids: Vec<IdAssignment>,
    /// Effective operations after ID assignment, used for forward revision logs/SSE.
    #[serde(default, skip)]
    #[schemars(skip)]
    pub applied_ops: Vec<PatchOp>,
}

/// Apply operations atomically on a cloned document.
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
                document.cover = cover.clone();
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
            PatchOp::SetTheme { theme } => {
                document.theme = theme.clone();
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
            | PatchOp::SetTheme { .. } => {}
        }
    }
}

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

fn apply_block_patch(block: &mut Block, patch: &BlockPatch) -> Result<(), PatchError> {
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
            if let Some(option_patch) = &patch.option_patch {
                chart.option_patch = Some(merge_optional_json(
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
            &["title", "numbering", "note", "append_rows", "update_cell"][..],
            "title, numbering, note, append_rows, update_cell",
        ),
        BlockKind::Echarts(_) => (
            &[
                "title",
                "numbering",
                "note",
                "option_patch",
                "fallback_summary",
            ][..],
            "title, numbering, note, option_patch, fallback_summary",
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
        ("option_patch", patch.option_patch.is_some()),
        ("append_rows", !patch.append_rows.is_empty()),
        ("update_cell", patch.update_cell.is_some()),
    ];
    let unsupported: Vec<&str> = present
        .iter()
        .filter(|(field, is_present)| *is_present && !supported.contains(field))
        .map(|(field, _)| *field)
        .collect();
    if unsupported.is_empty() {
        return Ok(());
    }
    if patch
        .option_patch
        .as_ref()
        .is_some_and(|value| !value.is_object())
    {
        return Err(PatchError::InvalidPatch {
            block_id: block.id.clone(),
            message: "option_patch must be a JSON object".to_string(),
        });
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

#[cfg(test)]
mod tests {
    use super::*;
    use crate::test_support::*;
    use crate::{
        block::{Block, ColumnNode, EchartsBlock, EchartsMode, TableBlock},
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
                    patch: BlockPatch {
                        text: Some("x".into()),
                        ..Default::default()
                    },
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
                patch: BlockPatch {
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
                },
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
    fn table_append_and_update_cell() {
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
                patch: BlockPatch {
                    title: Some("Revenue".into()),
                    numbering: Some(NumberingKind::Table),
                    note: Some("Audited".into()),
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
                },
            }],
        )
        .unwrap();
        let BlockKind::Table(table) = &doc.block("t").unwrap().body else {
            panic!()
        };
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
                mode: EchartsMode::Preset,
                preset: None,
                option: None,
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
                patch: BlockPatch {
                    option_patch: Some(serde_json::json!({"series":[{"data":[1,2]}]})),
                    fallback_summary: Some("Updated summary".into()),
                    ..Default::default()
                },
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
    }

    #[test]
    fn unsupported_patch_lists_supported_fields() {
        let mut doc = doc();
        let error = apply_operations_in_place(
            &mut doc,
            &[PatchOp::PatchBlock {
                block_id: block_id("a"),
                patch: BlockPatch {
                    update_cell: Some(CellUpdate {
                        row_index: 0,
                        column_key: "x".into(),
                        value: CellValue::Null,
                        summary: false,
                    }),
                    ..Default::default()
                },
            }],
        )
        .unwrap_err();
        assert!(matches!(error, PatchError::UnsupportedPatch { .. }));
        assert!(error.to_string().contains("text"));
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
}
