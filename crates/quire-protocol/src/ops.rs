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
    block::{
        Block, BlockKind, BlockStyle, CellValue, ColumnNode, NumberingKind, is_semantic_block_id,
    },
    echarts::merge_chart_option_patch,
    ids::{BlockId, SourceId},
    lint::LintConfig,
    model::{CoverConfig, ReferencesConfig, ReportDocument, SourceEntry, ThemeConfig, TocConfig},
    table::validate_table,
};

/// Partial style update. Omitted fields are preserved; empty string variants clear the field.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
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
#[serde(deny_unknown_fields)]
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
#[serde(deny_unknown_fields)]
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
    /// Set the block-level lint ignore list. Empty clears it.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub lint_ignore: Option<Vec<String>>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub style: Option<BlockStylePatch>,
    /// Replace the ECharts effective option wholesale, clearing any stored `option_patch`.
    /// Supply `option_patch` in the same patch to replace the base and then deep-merge a smaller
    /// overlay.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub option: Option<serde_json::Value>,
    /// Deep-merged into an ECharts block's existing `option_patch`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub option_patch: Option<serde_json::Value>,
    /// Replace the chart canvas height in pixels.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub height: Option<u32>,
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
    /// Replace a table's column schema in place (preserving `block_id` and comment anchors).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub columns: Option<Vec<ColumnNode>>,
    /// Replace a table's body rows in place (preserving `block_id` and comment anchors).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub rows: Option<Vec<BTreeMap<String, CellValue>>>,
    /// Replace a table's summary rows in place.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub summary_rows: Option<Vec<BTreeMap<String, CellValue>>>,
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
            && self.lint_ignore.is_none()
            && self.style.as_ref().is_none_or(BlockStylePatch::is_empty)
            && self.option.is_none()
            && self.option_patch.is_none()
            && self.height.is_none()
            && self.fallback_summary.is_none()
            && self.title.is_none()
            && self.numbering.is_none()
            && self.note.is_none()
            && self.fallback_text.is_none()
            && self.columns.is_none()
            && self.rows.is_none()
            && self.summary_rows.is_none()
            && self.append_rows.is_empty()
            && self.update_cell.is_none()
    }

    fn merge_with(mut self, nested: Option<Self>) -> Self {
        let Some(nested) = nested else {
            return self;
        };
        if nested.text.is_some() {
            self.text = nested.text;
        }
        if nested.level.is_some() {
            self.level = nested.level;
        }
        if nested.class_name.is_some() {
            self.class_name = nested.class_name;
        }
        if nested.source_ids.is_some() {
            self.source_ids = nested.source_ids;
        }
        if nested.source_note.is_some() {
            self.source_note = nested.source_note;
        }
        if nested.lint_ignore.is_some() {
            self.lint_ignore = nested.lint_ignore;
        }
        if nested.style.is_some() {
            self.style = nested.style;
        }
        if nested.option.is_some() {
            self.option = nested.option;
        }
        if nested.option_patch.is_some() {
            self.option_patch = nested.option_patch;
        }
        if nested.height.is_some() {
            self.height = nested.height;
        }
        if nested.fallback_summary.is_some() {
            self.fallback_summary = nested.fallback_summary;
        }
        if nested.title.is_some() {
            self.title = nested.title;
        }
        if nested.numbering.is_some() {
            self.numbering = nested.numbering;
        }
        if nested.note.is_some() {
            self.note = nested.note;
        }
        if nested.fallback_text.is_some() {
            self.fallback_text = nested.fallback_text;
        }
        if nested.columns.is_some() {
            self.columns = nested.columns;
        }
        if nested.rows.is_some() {
            self.rows = nested.rows;
        }
        if nested.summary_rows.is_some() {
            self.summary_rows = nested.summary_rows;
        }
        if !nested.append_rows.is_empty() {
            self.append_rows.extend(nested.append_rows);
        }
        if nested.update_cell.is_some() {
            self.update_cell = nested.update_cell;
        }
        self
    }
}

/// Forgiving wire format for `PatchOp` that accepts `PatchBlock` fields either nested under
/// `"patch": {...}`, flattened directly at the top level of `"type": "patch_block"`, or both.
#[allow(clippy::large_enum_variant)]
#[derive(Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
enum RawPatchOp {
    InsertAfter {
        target_id: BlockId,
        blocks: Vec<Block>,
    },
    InsertBefore {
        target_id: BlockId,
        blocks: Vec<Block>,
    },
    Append {
        blocks: Vec<Block>,
    },
    AppendToSection {
        section_id: BlockId,
        blocks: Vec<Block>,
    },
    PatchBlock {
        block_id: BlockId,
        #[serde(default)]
        patch: Option<BlockPatch>,
        #[serde(default)]
        text: Option<String>,
        #[serde(default)]
        level: Option<u8>,
        #[serde(default)]
        class_name: Option<String>,
        #[serde(default)]
        source_ids: Option<Vec<SourceId>>,
        #[serde(default)]
        source_note: Option<String>,
        #[serde(default)]
        lint_ignore: Option<Vec<String>>,
        #[serde(default)]
        style: Option<BlockStylePatch>,
        #[serde(default)]
        option: Option<serde_json::Value>,
        #[serde(default)]
        option_patch: Option<serde_json::Value>,
        #[serde(default)]
        height: Option<u32>,
        #[serde(default)]
        fallback_summary: Option<String>,
        #[serde(default, alias = "caption")]
        title: Option<String>,
        #[serde(default)]
        numbering: Option<NumberingKind>,
        #[serde(default)]
        note: Option<String>,
        #[serde(default)]
        fallback_text: Option<String>,
        #[serde(default)]
        columns: Option<Vec<ColumnNode>>,
        #[serde(default)]
        rows: Option<Vec<BTreeMap<String, CellValue>>>,
        #[serde(default)]
        summary_rows: Option<Vec<BTreeMap<String, CellValue>>>,
        #[serde(default)]
        append_rows: Vec<BTreeMap<String, CellValue>>,
        #[serde(default)]
        update_cell: Option<CellUpdate>,
    },
    ReplaceRange {
        start_id: BlockId,
        end_id: BlockId,
        blocks: Vec<Block>,
    },
    DeleteBlocks {
        block_ids: Vec<BlockId>,
    },
    MoveBlock {
        block_id: BlockId,
        after_id: Option<BlockId>,
    },
    SetCover {
        cover: CoverConfig,
    },
    SetToc {
        toc: TocConfig,
    },
    SetReferences {
        references: ReferencesConfig,
    },
    SetLint {
        lint: LintConfig,
    },
    SetTheme {
        theme: ThemeConfig,
    },
    SetSources {
        sources: IndexMap<SourceId, SourceEntry>,
    },
}

impl From<RawPatchOp> for PatchOp {
    fn from(raw: RawPatchOp) -> Self {
        match raw {
            RawPatchOp::InsertAfter { target_id, blocks } => {
                Self::InsertAfter { target_id, blocks }
            }
            RawPatchOp::InsertBefore { target_id, blocks } => {
                Self::InsertBefore { target_id, blocks }
            }
            RawPatchOp::Append { blocks } => Self::Append { blocks },
            RawPatchOp::AppendToSection { section_id, blocks } => {
                Self::AppendToSection { section_id, blocks }
            }
            RawPatchOp::PatchBlock {
                block_id,
                patch,
                text,
                level,
                class_name,
                source_ids,
                source_note,
                lint_ignore,
                style,
                option,
                option_patch,
                height,
                fallback_summary,
                title,
                numbering,
                note,
                fallback_text,
                columns,
                rows,
                summary_rows,
                append_rows,
                update_cell,
            } => {
                let inline = BlockPatch {
                    text,
                    level,
                    class_name,
                    source_ids,
                    source_note,
                    lint_ignore,
                    style,
                    option,
                    option_patch,
                    height,
                    fallback_summary,
                    title,
                    numbering,
                    note,
                    fallback_text,
                    columns,
                    rows,
                    summary_rows,
                    append_rows,
                    update_cell,
                };
                Self::PatchBlock {
                    block_id,
                    patch: inline.merge_with(patch),
                }
            }
            RawPatchOp::ReplaceRange {
                start_id,
                end_id,
                blocks,
            } => Self::ReplaceRange {
                start_id,
                end_id,
                blocks,
            },
            RawPatchOp::DeleteBlocks { block_ids } => Self::DeleteBlocks { block_ids },
            RawPatchOp::MoveBlock { block_id, after_id } => Self::MoveBlock { block_id, after_id },
            RawPatchOp::SetCover { cover } => Self::SetCover { cover },
            RawPatchOp::SetToc { toc } => Self::SetToc { toc },
            RawPatchOp::SetReferences { references } => Self::SetReferences { references },
            RawPatchOp::SetLint { lint } => Self::SetLint { lint },
            RawPatchOp::SetTheme { theme } => Self::SetTheme { theme },
            RawPatchOp::SetSources { sources } => Self::SetSources { sources },
        }
    }
}

/// A declarative mutation. Deserialized with a `type` discriminator.
///
/// The large `PatchBlock` variant is intentional: boxing it would force every JSON caller and
/// store adapter to unwrap an extra pointer for a cold error path.
#[allow(clippy::large_enum_variant)]
#[derive(Debug, Clone, PartialEq, Serialize, JsonSchema)]
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
    /// Replace the complete document lint policy.
    SetLint {
        /// Complete replacement lint config.
        lint: LintConfig,
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

impl<'de> Deserialize<'de> for PatchOp {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: serde::Deserializer<'de>,
    {
        RawPatchOp::deserialize(deserializer).map(Into::into)
    }
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
            | Self::SetLint { .. }
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
            | Self::SetLint { .. }
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
                | Self::SetLint { .. }
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
    /// Map of submitted block IDs to final IDs. Colliding or empty IDs are auto-assigned.
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
            PatchOp::SetLint { lint } => {
                document.lint = lint.clone();
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
            | PatchOp::SetLint { .. }
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

#[cfg(test)]
mod tests {
    use super::*;
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
                patch: BlockPatch {
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
                },
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
                patch: BlockPatch {
                    option: Some(serde_json::json!({
                        "dataset": {"source": [["Q", "V"], ["Q1", 10]]},
                        "series": [{"type": "bar", "data": [10]}]
                    })),
                    height: Some(480),
                    title: Some("Patched chart".into()),
                    ..Default::default()
                },
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
                patch: BlockPatch {
                    option: Some(serde_json::json!("not an object")),
                    ..Default::default()
                },
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
        let patch: BlockPatch =
            serde_json::from_value(serde_json::json!({"caption": "Legacy title"}))
                .expect("caption alias");
        assert_eq!(patch.title.as_deref(), Some("Legacy title"));

        let error = serde_json::from_value::<BlockPatch>(serde_json::json!({"titel": "typo"}))
            .expect_err("unknown block patch field");
        assert!(error.to_string().contains("titel"), "{error}");

        let error = serde_json::from_value::<BlockStylePatch>(
            serde_json::json!({"page_break_befor": true}),
        )
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
}
