//! Snapshot-backed incremental document synchronization for read-only frontends.
//!
//! Unlike [`crate::PatchOp`], this wire format contains already-applied changes. Clients therefore
//! never need to duplicate the canonical patch engine or its ID-assignment behavior.

use indexmap::IndexMap;
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

use crate::{
    Block, BlockId, ComputedDocumentMeta, CoverConfig, DocId, LintConfig, ReferencesConfig,
    ReportDocument, Revision, SourceEntry, SourceId, ThemeConfig, TocConfig, compute_document_meta,
};

/// Applied document changes between two committed revisions.
///
/// `block_ids` is always the complete target order. `upserted_blocks` contains only added or
/// modified blocks; unchanged blocks can be retained by the client. Optional configuration fields
/// are present only when their values changed. `computed` is projected from the target snapshot by
/// the canonical Rust implementation.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct DocumentSyncDelta {
    pub doc_id: DocId,
    pub from_revision: Revision,
    pub to_revision: Revision,
    pub block_ids: Vec<BlockId>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub upserted_blocks: Vec<Block>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub removed_block_ids: Vec<BlockId>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub cover: Option<CoverConfig>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub toc: Option<TocConfig>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub theme: Option<ThemeConfig>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub references: Option<ReferencesConfig>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub lint: Option<LintConfig>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[schemars(with = "Option<std::collections::BTreeMap<SourceId, SourceEntry>>")]
    pub sources: Option<IndexMap<SourceId, SourceEntry>>,
    pub computed: ComputedDocumentMeta,
}

/// Build an applied synchronization delta from canonical before/after snapshots.
pub fn sync_documents(before: &ReportDocument, after: &ReportDocument) -> DocumentSyncDelta {
    let before_by_id = before
        .blocks
        .iter()
        .map(|block| (&block.id, block))
        .collect::<std::collections::BTreeMap<_, _>>();
    let after_by_id = after
        .blocks
        .iter()
        .map(|block| &block.id)
        .collect::<std::collections::BTreeSet<_>>();

    DocumentSyncDelta {
        doc_id: after.doc_id.clone(),
        from_revision: before.revision,
        to_revision: after.revision,
        block_ids: after.blocks.iter().map(|block| block.id.clone()).collect(),
        upserted_blocks: after
            .blocks
            .iter()
            .filter(|block| before_by_id.get(&block.id) != Some(block))
            .cloned()
            .collect(),
        removed_block_ids: before
            .blocks
            .iter()
            .filter(|block| !after_by_id.contains(&block.id))
            .map(|block| block.id.clone())
            .collect(),
        cover: (before.cover != after.cover).then(|| after.cover.clone()),
        toc: (before.toc != after.toc).then(|| after.toc.clone()),
        theme: (before.theme != after.theme).then(|| after.theme.clone()),
        references: (before.references != after.references).then(|| after.references.clone()),
        lint: (before.lint != after.lint).then(|| after.lint.clone()),
        sources: (before.sources != after.sources).then(|| after.sources.clone()),
        computed: compute_document_meta(after),
    }
}

#[cfg(test)]
mod tests {
    use crate::test_support::{block_id, doc_id, revision, source_id};

    use super::*;

    #[test]
    fn sync_contains_only_applied_changes_and_exact_target_order() {
        let mut before = ReportDocument::new(doc_id("report_1"));
        before.revision = revision(2);
        before.blocks = vec![
            Block::paragraph(block_id("intro"), "Intro"),
            Block::paragraph(block_id("edited"), "Before"),
            Block::paragraph(block_id("removed"), "Remove me"),
        ];

        let mut after = ReportDocument::new(doc_id("report_1"));
        after.revision = revision(5);
        after.cover.title = "Updated".into();
        after.toc.max_depth = 2;
        after.theme.preset = "modern".into();
        after.references.enabled = false;
        after.lint.ignore = vec!["QR420".into()];
        after
            .sources
            .insert(source_id("source_1"), SourceEntry::new("Annual report"));
        after.blocks = vec![
            Block::paragraph(block_id("added"), "New"),
            Block::paragraph(block_id("edited"), "After"),
            Block::paragraph(block_id("intro"), "Intro"),
        ];

        let delta = sync_documents(&before, &after);
        assert_eq!(delta.from_revision, revision(2));
        assert_eq!(delta.to_revision, revision(5));
        assert_eq!(
            delta.block_ids,
            vec![block_id("added"), block_id("edited"), block_id("intro")]
        );
        assert_eq!(
            delta
                .upserted_blocks
                .iter()
                .map(|block| block.id.clone())
                .collect::<Vec<_>>(),
            vec![block_id("added"), block_id("edited")]
        );
        assert_eq!(delta.removed_block_ids, vec![block_id("removed")]);
        assert_eq!(delta.cover, Some(after.cover.clone()));
        assert_eq!(delta.toc, Some(after.toc.clone()));
        assert_eq!(delta.theme, Some(after.theme.clone()));
        assert_eq!(delta.references, Some(after.references.clone()));
        assert_eq!(delta.lint, Some(after.lint.clone()));
        assert_eq!(delta.sources, Some(after.sources.clone()));
        assert_eq!(delta.computed, compute_document_meta(&after));
    }

    #[test]
    fn unchanged_blocks_and_configuration_are_not_duplicated() {
        let mut before = ReportDocument::new(doc_id("report_1"));
        before.revision = revision(3);
        before.blocks = vec![Block::paragraph(block_id("intro"), "Same")];
        let mut after = before.clone();
        after.revision = revision(4);

        let delta = sync_documents(&before, &after);
        assert!(delta.upserted_blocks.is_empty());
        assert!(delta.removed_block_ids.is_empty());
        assert_eq!(delta.block_ids, vec![block_id("intro")]);
        assert!(delta.cover.is_none());
        assert!(delta.toc.is_none());
        assert!(delta.theme.is_none());
        assert!(delta.references.is_none());
        assert!(delta.lint.is_none());
        assert!(delta.sources.is_none());
        assert_eq!(delta.computed, compute_document_meta(&after));
    }
}
