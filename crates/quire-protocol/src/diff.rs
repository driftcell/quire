//! Structured block-level diffs between two committed document snapshots.

use std::collections::BTreeMap;

use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

use crate::{Block, BlockId, DocId, ReportDocument, Revision};

/// One block-level change between two snapshots.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum BlockDiffEntry {
    Added {
        block: Box<Block>,
    },
    Removed {
        block: Box<Block>,
    },
    Modified {
        block_id: BlockId,
        before: Box<Block>,
        after: Box<Block>,
    },
}

/// Non-block document fields that changed between two snapshots.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
pub struct DocumentMetadataDiff {
    pub cover_changed: bool,
    pub toc_changed: bool,
    pub theme_changed: bool,
    pub references_changed: bool,
    pub sources_changed: bool,
}

impl DocumentMetadataDiff {
    pub fn any(&self) -> bool {
        self.cover_changed
            || self.toc_changed
            || self.theme_changed
            || self.references_changed
            || self.sources_changed
    }
}

/// Full block-level diff returned by `GET /api/docs/{doc_id}/diff`.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct DocumentDiffReport {
    pub doc_id: DocId,
    pub from_revision: Revision,
    pub to_revision: Revision,
    pub blocks: Vec<BlockDiffEntry>,
    #[serde(flatten)]
    pub metadata: DocumentMetadataDiff,
    /// True when surviving blocks changed relative order.
    pub order_changed: bool,
}

impl DocumentDiffReport {
    pub fn added(&self) -> usize {
        self.blocks
            .iter()
            .filter(|entry| matches!(entry, BlockDiffEntry::Added { .. }))
            .count()
    }

    pub fn removed(&self) -> usize {
        self.blocks
            .iter()
            .filter(|entry| matches!(entry, BlockDiffEntry::Removed { .. }))
            .count()
    }

    pub fn modified(&self) -> usize {
        self.blocks
            .iter()
            .filter(|entry| matches!(entry, BlockDiffEntry::Modified { .. }))
            .count()
    }
}

/// Compare two document snapshots by block ID and document metadata.
///
/// Added/modified entries follow the target snapshot order; removals follow the source snapshot
/// order. Reordering of surviving blocks is reported through `order_changed` rather than as a
/// block modification.
pub fn diff_documents(before: &ReportDocument, after: &ReportDocument) -> DocumentDiffReport {
    let before_by_id: BTreeMap<BlockId, &Block> = before
        .blocks
        .iter()
        .map(|block| (block.id.clone(), block))
        .collect();
    let after_by_id: BTreeMap<BlockId, &Block> = after
        .blocks
        .iter()
        .map(|block| (block.id.clone(), block))
        .collect();

    let mut blocks = Vec::new();
    for block in &after.blocks {
        match before_by_id.get(&block.id) {
            None => blocks.push(BlockDiffEntry::Added {
                block: Box::new(block.clone()),
            }),
            Some(before_block) if *before_block != block => {
                blocks.push(BlockDiffEntry::Modified {
                    block_id: block.id.clone(),
                    before: Box::new((*before_block).clone()),
                    after: Box::new(block.clone()),
                });
            }
            Some(_) => {}
        }
    }
    for block in &before.blocks {
        if !after_by_id.contains_key(&block.id) {
            blocks.push(BlockDiffEntry::Removed {
                block: Box::new(block.clone()),
            });
        }
    }

    let before_order: Vec<&BlockId> = before
        .blocks
        .iter()
        .filter(|block| after_by_id.contains_key(&block.id))
        .map(|block| &block.id)
        .collect();
    let after_order: Vec<&BlockId> = after
        .blocks
        .iter()
        .filter(|block| before_by_id.contains_key(&block.id))
        .map(|block| &block.id)
        .collect();

    DocumentDiffReport {
        doc_id: after.doc_id.clone(),
        from_revision: before.revision,
        to_revision: after.revision,
        blocks,
        metadata: DocumentMetadataDiff {
            cover_changed: before.cover != after.cover,
            toc_changed: before.toc != after.toc,
            theme_changed: before.theme != after.theme,
            references_changed: before.references != after.references,
            sources_changed: before.sources != after.sources,
        },
        order_changed: before_order != after_order,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::test_support::{block_id, doc_id, revision};

    #[test]
    fn diff_reports_added_removed_modified_and_metadata() {
        let mut before = ReportDocument::new(doc_id("report_1"));
        before.revision = revision(2);
        before.blocks = vec![
            Block::paragraph(block_id("intro"), "Intro"),
            Block::paragraph(block_id("old"), "Old"),
            Block::paragraph(block_id("edited"), "Before"),
        ];
        let mut after = ReportDocument::new(doc_id("report_1"));
        after.revision = revision(5);
        after.cover.title = "Updated".into();
        after.blocks = vec![
            Block::paragraph(block_id("intro"), "Intro"),
            Block::paragraph(block_id("edited"), "After"),
            Block::paragraph(block_id("new"), "New"),
        ];

        let report = diff_documents(&before, &after);
        assert_eq!(report.from_revision, revision(2));
        assert_eq!(report.to_revision, revision(5));
        assert_eq!(report.added(), 1);
        assert_eq!(report.removed(), 1);
        assert_eq!(report.modified(), 1);
        assert!(report.metadata.cover_changed);
        assert!(!report.metadata.sources_changed);
        assert!(!report.order_changed);
        assert!(matches!(
            report.blocks.first(),
            Some(BlockDiffEntry::Modified { block_id, .. }) if block_id == &crate::test_support::block_id("edited")
        ));
        assert!(matches!(
            report.blocks.get(1),
            Some(BlockDiffEntry::Added { .. })
        ));
        assert!(matches!(
            report.blocks.get(2),
            Some(BlockDiffEntry::Removed { .. })
        ));
    }

    #[test]
    fn diff_detects_reordering_without_modification() {
        let mut before = ReportDocument::new(doc_id("report_1"));
        before.blocks = vec![
            Block::paragraph(block_id("a"), "A"),
            Block::paragraph(block_id("b"), "B"),
        ];
        let mut after = before.clone();
        after.blocks.reverse();
        let report = diff_documents(&before, &after);
        assert!(report.blocks.is_empty());
        assert!(report.order_changed);
    }

    #[test]
    fn diff_entry_stack_size_is_bounded() {
        assert!(std::mem::size_of::<BlockDiffEntry>() <= 96);
    }
}
