use super::*;

pub(crate) fn affected_block_ids(outcome: &PatchOutcome, diff: &DocumentDiff) -> Vec<String> {
    let mut ids: BTreeSet<BlockId> = outcome.affected_block_ids.iter().cloned().collect();
    ids.extend(diff.deleted_block_ids.iter().cloned());
    ids.extend(diff.changed_blocks.iter().map(|block| block.id.clone()));
    let ids: Vec<BlockId> = ids.into_iter().collect();
    block_ids_to_strings(&ids)
}

pub(crate) fn block_ids_to_strings(ids: &[BlockId]) -> Vec<String> {
    ids.iter().map(ToString::to_string).collect()
}

pub(crate) fn source_ids_to_strings(ids: &[SourceId]) -> Vec<String> {
    ids.iter().map(ToString::to_string).collect()
}

pub(crate) fn inline_warnings(document: &ReportDocument) -> InlineWarnings {
    lint_document(document).diagnostics
}

pub(crate) fn parse_json<T: DeserializeOwned>(value: Value, path: &str) -> Result<T, StoreError> {
    serde_json::from_value(value)
        .map_err(|error| StoreError::Internal(format!("invalid JSON at {path}: {error}")))
}

pub(crate) fn is_unique_violation(error: &sqlx::Error) -> bool {
    error
        .as_database_error()
        .and_then(|error| error.code())
        .is_some_and(|code| code == "23505")
}

pub fn validate_doc_id(doc_id: &str) -> Result<(), StoreError> {
    let valid = !doc_id.is_empty()
        && doc_id.len() <= 64
        && doc_id
            .chars()
            .next()
            .is_some_and(|ch| ch.is_ascii_alphanumeric())
        && doc_id
            .chars()
            .all(|ch| ch.is_ascii_alphanumeric() || matches!(ch, '-' | '_'));
    if valid {
        Ok(())
    } else {
        Err(StoreError::InvalidDocId {
            doc_id: doc_id.to_string(),
            reason: "use 1-64 ASCII letters/digits plus '-' '_', starting alphanumeric".into(),
        })
    }
}

pub fn validate_session_id(session_id: &str) -> Result<(), StoreError> {
    let valid = !session_id.is_empty()
        && session_id.len() <= 64
        && session_id
            .chars()
            .all(|ch| ch.is_ascii_alphanumeric() || matches!(ch, '-' | '_' | '.'));
    if valid {
        Ok(())
    } else {
        Err(StoreError::InvalidRequest(format!(
            "invalid session_id `{session_id}`; use 1-64 letters/digits plus '-' '_' '.'"
        )))
    }
}

pub fn validate_source_id(source_id: &str) -> Result<(), StoreError> {
    let valid = !source_id.is_empty()
        && source_id.len() <= 64
        && source_id
            .chars()
            .next()
            .is_some_and(|ch| ch.is_ascii_alphanumeric())
        && source_id
            .chars()
            .all(|ch| ch.is_ascii_alphanumeric() || matches!(ch, '-' | '_' | '.'));
    if valid {
        Ok(())
    } else {
        Err(StoreError::InvalidSourceId {
            source_id: source_id.to_string(),
            reason: "use 1-64 ASCII letters/digits plus '-' '_' '.', starting alphanumeric".into(),
        })
    }
}

pub(crate) type SourceMergeResult = (
    IndexMap<SourceId, SourceEntry>,
    BTreeMap<SourceId, SourceId>,
);

#[cfg(test)]
mod tests {
    use super::*;
    use quire_protocol::{Block, BlockId, CoverConfig, DocId, Rank, ReportDocument, SourceId};

    fn bid(raw: &str) -> BlockId {
        BlockId::parse(raw).expect("valid block id")
    }

    fn sid(raw: &str) -> SourceId {
        SourceId::parse(raw).expect("valid source id")
    }

    fn sample(blocks: Vec<Block>) -> ReportDocument {
        let mut doc = ReportDocument::new(DocId::parse("sample").expect("doc id"));
        doc.cover = CoverConfig {
            enabled: true,
            title: "Sample".into(),
            ..Default::default()
        };
        doc.blocks = blocks;
        doc
    }

    #[test]
    fn inverse_ops_restore_metadata_blocks_and_order() {
        let before = sample(vec![
            Block::paragraph(bid("a"), "Alpha"),
            Block::paragraph(bid("b"), "Beta"),
            Block::paragraph(bid("c"), "Gamma"),
        ]);
        let mut after = before.clone();
        after.cover.title = "Changed".into();
        after.blocks = vec![
            Block::paragraph(bid("c"), "Gamma changed"),
            Block::paragraph(bid("x"), "Inserted"),
            Block::paragraph(bid("a"), "Alpha"),
        ];
        let ops = generate_inverse_ops(&before, &after).unwrap();
        let mut restored = after.clone();
        apply_operations_in_place(&mut restored, &ops).unwrap();
        assert_eq!(restored.blocks, before.blocks);
        assert_eq!(restored.cover, before.cover);
    }

    #[test]
    fn rank_assignment_only_touches_new_or_moved_blocks() {
        let existing = BTreeMap::from([
            (bid("a"), Rank::parse("A").expect("rank")),
            (bid("b"), Rank::parse("B").expect("rank")),
            (bid("c"), Rank::parse("C").expect("rank")),
        ]);
        let after = sample(vec![
            Block::paragraph(bid("a"), "Alpha"),
            Block::paragraph(bid("new"), "New"),
            Block::paragraph(bid("b"), "Beta"),
        ]);
        let needs = HashSet::from(["new"]);
        let ranks = assign_new_ranks(&after, &existing, &needs).unwrap();
        assert_eq!(ranks.get(&bid("a")).map(Rank::as_str), Some("A"));
        assert_eq!(ranks.get(&bid("b")).map(Rank::as_str), Some("B"));
        assert!(
            ranks
                .get(&bid("new"))
                .is_some_and(|rank| rank.as_str() > "A" && rank.as_str() < "B")
        );
    }

    #[test]
    fn source_normalization_deduplicates_url_titles() {
        let mut doc = sample(vec![]);
        doc.sources.insert(
            sid("a"),
            SourceEntry {
                url: Some("https://Example.com/report/".into()),
                ..SourceEntry::new("One")
            },
        );
        doc.sources.insert(
            sid("b"),
            SourceEntry {
                url: Some("https://example.com/report".into()),
                ..SourceEntry::new("Two")
            },
        );
        assert!(validate_source_registry(&doc.sources).is_err());
        let mut source = SourceEntry::new("Revenue  Overview!");
        assert_eq!(normalize_title(&source.title), "revenue overview");
        source.url = None;
        assert_eq!(normalized_source_key(&source), "title:revenue overview");
    }
}
