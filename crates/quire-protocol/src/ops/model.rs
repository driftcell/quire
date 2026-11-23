use super::*;

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

    pub(crate) fn merge_with(mut self, nested: Option<Self>) -> Self {
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

/// A declarative mutation. Deserialized with a `type` discriminator.
///
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
        patch: Box<BlockPatch>,
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
        cover: Box<CoverConfig>,
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
        theme: Box<ThemeConfig>,
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
