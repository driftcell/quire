use super::*;

#[derive(Deserialize)]
pub(crate) struct RawPatchBlock {
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
}

/// Forgiving wire format for `PatchOp` that accepts `PatchBlock` fields either nested under
/// `"patch": {...}`, flattened directly at the top level of `"type": "patch_block"`, or both.
#[derive(Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub(crate) enum RawPatchOp {
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
    PatchBlock(Box<RawPatchBlock>),
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
        cover: Box<CoverConfig>,
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
        theme: Box<ThemeConfig>,
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
            RawPatchOp::PatchBlock(raw) => {
                let RawPatchBlock {
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
                } = *raw;
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
                    patch: Box::new(inline.merge_with(patch)),
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

impl<'de> Deserialize<'de> for PatchOp {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: serde::Deserializer<'de>,
    {
        RawPatchOp::deserialize(deserializer).map(Into::into)
    }
}
