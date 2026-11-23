use super::*;

pub(crate) fn assign_new_ranks(
    after: &ReportDocument,
    existing: &BTreeMap<BlockId, Rank>,
    needs_new_rank: &HashSet<&str>,
) -> Result<BTreeMap<BlockId, Rank>, StoreError> {
    let is_fixed = |block: &Block| {
        !needs_new_rank.contains(block.id.as_str()) && existing.contains_key(&block.id)
    };
    let mut ranks: BTreeMap<BlockId, Rank> = BTreeMap::new();
    let mut previous: Option<Rank> = None;
    let mut index = 0usize;
    while let Some(block) = after.blocks.get(index) {
        if is_fixed(block) {
            let rank = existing.get(&block.id).cloned().ok_or_else(|| {
                StoreError::Internal(format!("missing existing rank for `{}`", block.id))
            })?;
            if let Some(previous) = &previous
                && previous >= &rank
            {
                return Err(StoreError::RankExhausted(format!(
                    "existing rank `{rank}` is not after `{previous}`"
                )));
            }
            ranks.insert(block.id.clone(), rank.clone());
            previous = Some(rank);
            index += 1;
            continue;
        }

        let mut run = Vec::new();
        while let Some(candidate) = after.blocks.get(index) {
            if is_fixed(candidate) {
                break;
            }
            run.push(candidate);
            index += 1;
        }
        let next_fixed = after
            .blocks
            .iter()
            .skip(index)
            .find(|candidate| is_fixed(candidate))
            .and_then(|candidate| existing.get(&candidate.id))
            .cloned();
        let mut last = previous.clone();
        for block in run {
            let rank_text = if let Some(next) = &next_fixed {
                try_rank_between(last.as_ref().map(Rank::as_str), Some(next.as_str())).map_err(
                    |error| {
                        StoreError::RankExhausted(format!(
                            "between {:?} and {:?}: {error}",
                            last.as_ref().map(Rank::as_str),
                            next.as_str()
                        ))
                    },
                )?
            } else {
                rank_after(last.as_ref().map(Rank::as_str))
            };
            let rank = Rank::parse(rank_text)?;
            if let Some(last) = &last
                && last >= &rank
            {
                return Err(StoreError::RankExhausted(format!(
                    "generated rank `{rank}` is not after `{last}`"
                )));
            }
            ranks.insert(block.id.clone(), rank.clone());
            last = Some(rank);
        }
        previous = last;
    }

    let ordered: Vec<&Rank> = after
        .blocks
        .iter()
        .filter_map(|block| ranks.get(&block.id))
        .collect();
    for pair in ordered.windows(2) {
        if let (Some(left), Some(right)) = (pair.first(), pair.get(1))
            && left >= right
        {
            return Err(StoreError::RankExhausted(format!(
                "final ranks are not strictly ordered: {left} >= {right}"
            )));
        }
    }
    Ok(ranks)
}

pub(crate) async fn insert_block_row(
    tx: &mut Transaction<'_, Postgres>,
    doc_id: &str,
    block: &Block,
    rank: &str,
    revision: Revision,
) -> Result<(), StoreError> {
    let source_ids = source_ids_to_strings(&block.source_ids);
    sqlx::query!(
        r#"INSERT INTO report_blocks
           (doc_id, block_id, rank, type, payload, style, source_ids, updated_rev)
           VALUES ($1, $2, $3, $4, $5, $6, $7, $8)"#,
        doc_id,
        block.id.as_str(),
        rank,
        block.block_type(),
        serde_json::to_value(block)?,
        serde_json::to_value(&block.style)?,
        source_ids.as_slice(),
        revision.get()
    )
    .execute(&mut **tx)
    .await?;
    Ok(())
}

pub(crate) async fn persist_blocks(
    tx: &mut Transaction<'_, Postgres>,
    doc_id: &str,
    diff: &DocumentDiff,
    after: &ReportDocument,
    revision: Revision,
) -> Result<(), StoreError> {
    if !diff.deleted_block_ids.is_empty() {
        let deleted = block_ids_to_strings(&diff.deleted_block_ids);
        sqlx::query!(
            "DELETE FROM report_blocks WHERE doc_id = $1 AND block_id = ANY($2)",
            doc_id,
            deleted.as_slice()
        )
        .execute(&mut **tx)
        .await?;
    }
    for block in &diff.changed_blocks {
        let source_ids = source_ids_to_strings(&block.source_ids);
        let rank = diff.changed_ranks.get(&block.id).cloned().ok_or_else(|| {
            StoreError::Internal(format!(
                "changed block `{}` has no rank assignment",
                block.id
            ))
        })?;
        sqlx::query!(
            r#"INSERT INTO report_blocks
               (doc_id, block_id, rank, type, payload, style, source_ids, updated_rev)
               VALUES ($1, $2, $3, $4, $5, $6, $7, $8)
               ON CONFLICT (doc_id, block_id)
               DO UPDATE SET rank = EXCLUDED.rank,
                             type = EXCLUDED.type,
                             payload = EXCLUDED.payload,
                             style = EXCLUDED.style,
                             source_ids = EXCLUDED.source_ids,
                             updated_rev = EXCLUDED.updated_rev"#,
            doc_id,
            block.id.as_str(),
            rank.as_str(),
            block.block_type(),
            serde_json::to_value(block)?,
            serde_json::to_value(&block.style)?,
            source_ids.as_slice(),
            revision.get()
        )
        .execute(&mut **tx)
        .await?;
    }
    let _ = after;
    Ok(())
}

pub(crate) async fn update_document_row(
    tx: &mut Transaction<'_, Postgres>,
    doc_id: &str,
    document: &ReportDocument,
    revision: Revision,
) -> Result<(), StoreError> {
    sqlx::query!(
        r#"UPDATE report_documents
           SET current_rev = $2, cover = $3, toc = $4, theme = $5,
               "references" = $6, lint = $7, updated_at = NOW()
           WHERE doc_id = $1"#,
        doc_id,
        revision.get(),
        serde_json::to_value(&document.cover)?,
        serde_json::to_value(&document.toc)?,
        serde_json::to_value(&document.theme)?,
        serde_json::to_value(&document.references)?,
        serde_json::to_value(&document.lint)?
    )
    .execute(&mut **tx)
    .await?;
    Ok(())
}

pub(crate) async fn persist_sources(
    tx: &mut Transaction<'_, Postgres>,
    doc_id: &str,
    before: &ReportDocument,
    after: &ReportDocument,
) -> Result<(), StoreError> {
    if before.sources == after.sources {
        return Ok(());
    }
    // Source sets are small. Replacing the registry atomically avoids unique-index transient
    // collisions when normalized keys are swapped or rewritten by a rollback.
    sqlx::query!("DELETE FROM report_sources WHERE doc_id = $1", doc_id)
        .execute(&mut **tx)
        .await?;
    for (source_id, source) in &after.sources {
        sqlx::query!(
            r#"INSERT INTO report_sources (doc_id, source_id, normalized_key, metadata, excerpt)
               VALUES ($1, $2, $3, $4, $5)"#,
            doc_id,
            source_id.as_str(),
            normalized_source_key(source),
            serde_json::to_value(source)?,
            source.excerpt.as_deref()
        )
        .execute(&mut **tx)
        .await?;
    }
    Ok(())
}
