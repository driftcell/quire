use super::*;

impl PgReportStore {
    pub async fn apply_operations(
        &self,
        doc_id: &DocId,
        expected_revision: Revision,
        actor: &str,
        summary: &str,
        ops: &[PatchOp],
    ) -> Result<ApplyResult, StoreError> {
        self.apply_operations_with_sources(
            doc_id,
            expected_revision,
            actor,
            summary,
            ops,
            Vec::new(),
        )
        .await
    }

    /// Apply blocks and optional source upserts in the same transaction.
    pub async fn apply_operations_with_sources(
        &self,
        doc_id: &DocId,
        expected_revision: Revision,
        actor: &str,
        summary: &str,
        ops: &[PatchOp],
        sources: Vec<SourceUpsert>,
    ) -> Result<ApplyResult, StoreError> {
        self.apply_operations_with_extras(
            doc_id,
            expected_revision,
            actor,
            summary,
            ops,
            CommitExtras {
                sources,
                cleared_preview_ids: Vec::new(),
            },
        )
        .await
    }

    /// Apply blocks, optional source upserts and ephemeral preview clears in one transaction.
    pub async fn apply_operations_with_extras(
        &self,
        doc_id: &DocId,
        expected_revision: Revision,
        actor: &str,
        summary: &str,
        ops: &[PatchOp],
        extras: CommitExtras,
    ) -> Result<ApplyResult, StoreError> {
        let CommitExtras {
            sources,
            cleared_preview_ids,
        } = extras;
        let _guard = self.document_guard(doc_id.as_str()).await;
        let mut tx = self.pool.begin().await?;
        let current_rev = lock_current_revision(&mut tx, doc_id.as_str()).await?;
        if expected_revision > current_rev {
            return Err(StoreError::RevisionAhead {
                doc_id: doc_id.clone(),
                expected: expected_revision,
                current: current_rev,
            });
        }

        let loaded = load_document_in_tx(&mut tx, doc_id.as_str()).await?;
        let before = loaded.document.clone();
        let mut effective_ops = ops.to_vec();
        if !sources.is_empty() {
            let (merged_sources, canonical_map) = merge_source_upserts(&before.sources, &sources)?;
            remap_operations_sources(&mut effective_ops, &canonical_map);
            if merged_sources != before.sources {
                effective_ops.insert(
                    0,
                    PatchOp::SetSources {
                        sources: merged_sources,
                    },
                );
            }
        }

        if expected_revision < current_rev
            && let Some(conflict) = detect_conflict(
                &mut tx,
                doc_id.as_str(),
                expected_revision,
                current_rev,
                &effective_ops,
            )
            .await?
        {
            return Err(StoreError::Conflict(Box::new(conflict)));
        }

        let mut after = before.clone();
        let outcome = apply_operations_in_place(&mut after, &effective_ops)?;
        let normalization_warnings = normalize_document(&mut after);
        validate_source_registry(&after.sources)?;
        let mut warnings = normalization_warnings;
        warnings.extend(inline_warnings(&after));
        if outcome.no_op && after == before {
            return Ok(ApplyResult {
                new_revision: current_rev,
                base_revision: expected_revision,
                rebased: expected_revision < current_rev,
                rebased_over: usize::try_from(current_rev.get() - expected_revision.get())
                    .unwrap_or(0),
                outcome,
                inline_warnings: warnings,
            });
        }
        let new_revision = current_rev.next();
        after.revision = new_revision;

        let forward_ops = ops_to_json(&outcome.applied_ops)?;
        let inverse_ops = ops_to_json(&generate_inverse_ops(&before, &after)?)?;
        let diff = compute_document_diff(&before, &after, &loaded.ranks, &outcome)?;
        let affected_block_ids = affected_block_ids(&outcome, &diff);
        persist_blocks(&mut tx, doc_id.as_str(), &diff, &after, new_revision).await?;
        persist_sources(&mut tx, doc_id.as_str(), &before, &after).await?;
        update_document_row(&mut tx, doc_id.as_str(), &after, new_revision).await?;
        sqlx::query!(
            r#"INSERT INTO report_revisions
               (doc_id, rev_num, actor, summary, forward_ops, inverse_ops, affected_block_ids)
               VALUES ($1, $2, $3, $4, $5, $6, $7)"#,
            doc_id.as_str(),
            new_revision.get(),
            actor,
            summary,
            forward_ops,
            inverse_ops,
            &affected_block_ids
        )
        .execute(&mut *tx)
        .await?;
        tx.commit().await?;

        let event = RevisionEvent::new(doc_id.clone(), new_revision, actor, summary, forward_ops)
            .with_cleared_preview_ids(cleared_preview_ids);
        if let Err(error) = self.bus.publish_revision(&event).await {
            warn!(%error, doc_id = doc_id.as_str(), revision = %new_revision, "failed to publish revision");
        }
        self.maybe_snapshot(doc_id.as_str(), new_revision, after.clone());

        Ok(ApplyResult {
            new_revision,
            base_revision: expected_revision,
            rebased: expected_revision < current_rev,
            rebased_over: usize::try_from(current_rev.get() - expected_revision.get()).unwrap_or(0),
            outcome,
            inline_warnings: warnings,
        })
    }

    /// Roll back by replaying inverse operations in reverse revision order, into a new revision.
    pub async fn rollback_to_revision(
        &self,
        doc_id: &DocId,
        target_revision: Revision,
        actor: &str,
    ) -> Result<RollbackResult, StoreError> {
        let _guard = self.document_guard(doc_id.as_str()).await;
        let mut tx = self.pool.begin().await?;
        let current_rev = lock_current_revision(&mut tx, doc_id.as_str()).await?;
        if target_revision.is_zero() || target_revision >= current_rev {
            return Err(StoreError::InvalidRollbackTarget {
                doc_id: doc_id.clone(),
                target: target_revision,
                current: current_rev,
            });
        }
        let revision_rows = sqlx::query!(
            r#"SELECT rev_num, inverse_ops FROM report_revisions
               WHERE doc_id = $1 AND rev_num > $2 AND rev_num <= $3
               ORDER BY rev_num DESC"#,
            doc_id.as_str(),
            target_revision.get(),
            current_rev.get()
        )
        .fetch_all(&mut *tx)
        .await?;
        if revision_rows.len() as i64 != current_rev.get() - target_revision.get() {
            return Err(StoreError::CorruptRevision {
                doc_id: doc_id.clone(),
                revision: target_revision,
                message: "missing revision rows in rollback range".into(),
            });
        }

        let mut replay_ops = Vec::new();
        for row in &revision_rows {
            let ops: Vec<PatchOp> =
                serde_json::from_value(row.inverse_ops.clone()).map_err(|error| {
                    StoreError::CorruptRevision {
                        doc_id: doc_id.clone(),
                        revision: Revision::new(row.rev_num).unwrap_or(target_revision),
                        message: error.to_string(),
                    }
                })?;
            replay_ops.extend(ops);
        }

        let loaded = load_document_in_tx(&mut tx, doc_id.as_str()).await?;
        let before = loaded.document.clone();
        let mut after = before.clone();
        let outcome = apply_operations_in_place(&mut after, &replay_ops)?;
        let normalization_warnings = normalize_document(&mut after);
        validate_source_registry(&after.sources)?;
        let new_revision = current_rev.next();
        after.revision = new_revision;

        let forward_ops = ops_to_json(&replay_ops)?;
        let inverse_ops = ops_to_json(&generate_inverse_ops(&after, &before)?)?;
        let diff = compute_document_diff(&before, &after, &loaded.ranks, &outcome)?;
        let affected_block_ids = affected_block_ids(&outcome, &diff);
        persist_blocks(&mut tx, doc_id.as_str(), &diff, &after, new_revision).await?;
        persist_sources(&mut tx, doc_id.as_str(), &before, &after).await?;
        update_document_row(&mut tx, doc_id.as_str(), &after, new_revision).await?;
        let summary = format!("rollback_to_revision {target_revision}");
        sqlx::query!(
            r#"INSERT INTO report_revisions
               (doc_id, rev_num, actor, summary, forward_ops, inverse_ops, affected_block_ids)
               VALUES ($1, $2, $3, $4, $5, $6, $7)"#,
            doc_id.as_str(),
            new_revision.get(),
            actor,
            summary,
            forward_ops,
            inverse_ops,
            &affected_block_ids
        )
        .execute(&mut *tx)
        .await?;
        tx.commit().await?;

        let mut warnings = normalization_warnings;
        warnings.extend(inline_warnings(&after));
        let event = RevisionEvent::new(doc_id.clone(), new_revision, actor, &summary, forward_ops);
        if let Err(error) = self.bus.publish_revision(&event).await {
            warn!(%error, doc_id = doc_id.as_str(), revision = %new_revision, "failed to publish rollback revision");
        }
        self.maybe_snapshot(doc_id.as_str(), new_revision, after);

        Ok(RollbackResult {
            new_revision,
            target_revision,
            replayed_revisions: revision_rows.len(),
            outcome,
            inline_warnings: warnings,
        })
    }
}

async fn detect_conflict(
    tx: &mut Transaction<'_, Postgres>,
    doc_id: &str,
    expected_rev: Revision,
    current_rev: Revision,
    incoming: &[PatchOp],
) -> Result<Option<RevisionConflict>, StoreError> {
    let typed_doc_id = DocId::parse(doc_id)?;
    let rows = sqlx::query!(
        r#"SELECT rev_num, forward_ops, affected_block_ids
           FROM report_revisions
           WHERE doc_id = $1 AND rev_num > $2 AND rev_num <= $3
           ORDER BY rev_num ASC"#,
        doc_id,
        expected_rev.get(),
        current_rev.get()
    )
    .fetch_all(&mut **tx)
    .await?;
    let mut incoming_ids: BTreeSet<BlockId> = incoming
        .iter()
        .flat_map(PatchOp::touched_block_ids)
        .collect();
    // A ReplaceRange logically touches every block between its endpoints. Expand the endpoints
    // against the current committed order so a concurrent insert inside the range cannot be
    // silently deleted by an apparently disjoint rebase.
    let ordered_ids: Vec<BlockId> = sqlx::query!(
        r#"SELECT block_id FROM report_blocks WHERE doc_id = $1 ORDER BY rank COLLATE "C""#,
        doc_id
    )
    .fetch_all(&mut **tx)
    .await?
    .into_iter()
    .map(|row| BlockId::parse(row.block_id))
    .collect::<Result<Vec<_>, _>>()?;
    for op in incoming {
        if let PatchOp::ReplaceRange {
            start_id, end_id, ..
        } = op
            && let (Some(start), Some(end)) = (
                ordered_ids.iter().position(|id| id == start_id),
                ordered_ids.iter().position(|id| id == end_id),
            )
            && start <= end
            && let Some(range) = ordered_ids.get(start..=end)
        {
            incoming_ids.extend(range.iter().cloned());
        }
    }
    let incoming_sources: Option<BTreeSet<SourceId>> = incoming.iter().find_map(|op| match op {
        PatchOp::SetSources { sources } => Some(sources.keys().cloned().collect()),
        _ => None,
    });
    let mut conflicting: BTreeSet<BlockId> = BTreeSet::new();
    let mut conflicting_sources: BTreeSet<SourceId> = BTreeSet::new();
    let mut conflicting_revisions: Vec<Revision> = Vec::new();
    for row in rows {
        let mut overlap: BTreeSet<BlockId> = row
            .affected_block_ids
            .iter()
            .filter_map(|block_id| BlockId::parse(block_id).ok())
            .filter(|block_id| incoming_ids.contains(block_id))
            .collect();
        let parsed_ops: Result<Vec<PatchOp>, _> = serde_json::from_value(row.forward_ops.clone());
        // Revisions written before migration 002 have an empty materialized list. Fall back to
        // parsing their forward ops so an upgrade cannot silently weaken Smart CAS.
        if row.affected_block_ids.is_empty() {
            match &parsed_ops {
                Ok(ops) => {
                    let touched: BTreeSet<BlockId> =
                        ops.iter().flat_map(PatchOp::touched_block_ids).collect();
                    overlap.extend(touched.intersection(&incoming_ids).cloned());
                }
                Err(_) => {
                    overlap.extend(incoming_ids.iter().cloned());
                }
            }
        }
        if !overlap.is_empty() {
            conflicting.extend(overlap);
            conflicting_revisions.push(Revision::new(row.rev_num)?);
        }
        if let Some(incoming_sources) = &incoming_sources {
            match &parsed_ops {
                Ok(ops) => {
                    for op in ops {
                        if let PatchOp::SetSources { sources } = op {
                            // SetSources replaces the whole registry. Any concurrent source
                            // replacement must be re-read and merged, even when key sets differ.
                            conflicting_sources.extend(
                                incoming_sources
                                    .intersection(&sources.keys().cloned().collect())
                                    .cloned(),
                            );
                            conflicting_revisions.push(Revision::new(row.rev_num)?);
                        }
                    }
                }
                Err(_) => {
                    conflicting_sources.extend(incoming_sources.iter().cloned());
                    conflicting_revisions.push(Revision::new(row.rev_num)?);
                }
            }
        }
    }
    conflicting_revisions.sort_unstable();
    conflicting_revisions.dedup();
    if conflicting.is_empty() && conflicting_sources.is_empty() && conflicting_revisions.is_empty()
    {
        return Ok(None);
    }
    let ids: Vec<BlockId> = conflicting.into_iter().collect();
    let ids_db = block_ids_to_strings(&ids);
    let block_rows = sqlx::query!(
        r#"SELECT payload FROM report_blocks
           WHERE doc_id = $1 AND block_id = ANY($2)
           ORDER BY rank COLLATE "C""#,
        doc_id,
        ids_db.as_slice()
    )
    .fetch_all(&mut **tx)
    .await?;
    let mut latest_blocks = Vec::new();
    for row in block_rows {
        latest_blocks.push(parse_json(row.payload, "report_blocks.payload")?);
    }
    Ok(Some(RevisionConflict {
        doc_id: typed_doc_id,
        expected_revision: expected_rev,
        current_revision: current_rev,
        conflicting_block_ids: ids,
        conflicting_source_ids: conflicting_sources.into_iter().collect(),
        latest_blocks,
        conflicting_revisions,
    }))
}

fn compute_document_diff(
    before: &ReportDocument,
    after: &ReportDocument,
    existing_ranks: &BTreeMap<BlockId, Rank>,
    outcome: &PatchOutcome,
) -> Result<DocumentDiff, StoreError> {
    let before_by_id: HashMap<&str, &Block> = before
        .blocks
        .iter()
        .map(|block| (block.id.as_str(), block))
        .collect();
    let after_ids: HashSet<&str> = after.blocks.iter().map(|block| block.id.as_str()).collect();
    let inserted: HashSet<&str> = outcome
        .inserted_block_ids
        .iter()
        .map(BlockId::as_str)
        .collect();
    let moved: HashSet<&str> = outcome
        .moved_block_ids
        .iter()
        .map(BlockId::as_str)
        .collect();
    let needs_new_rank: HashSet<&str> = inserted.union(&moved).copied().collect();

    let rank_map = assign_new_ranks(after, existing_ranks, &needs_new_rank)?;
    let mut deleted_block_ids = before
        .blocks
        .iter()
        .filter(|block| !after_ids.contains(block.id.as_str()))
        .map(|block| block.id.clone())
        .collect::<Vec<_>>();
    deleted_block_ids.sort();

    let mut changed_blocks = Vec::new();
    let mut changed_ranks = BTreeMap::new();
    for block in &after.blocks {
        let needs_rank = needs_new_rank.contains(block.id.as_str());
        let content_changed = before_by_id
            .get(block.id.as_str())
            .is_none_or(|before_block| *before_block != block);
        if needs_rank || content_changed {
            changed_blocks.push(block.clone());
        }
        if needs_rank {
            let rank = rank_map.get(&block.id).cloned().ok_or_else(|| {
                StoreError::Internal(format!("missing assigned rank for `{}`", block.id))
            })?;
            changed_ranks.insert(block.id.clone(), rank);
        } else if content_changed && let Some(rank) = existing_ranks.get(&block.id) {
            changed_ranks.insert(block.id.clone(), rank.clone());
        }
    }
    Ok(DocumentDiff {
        deleted_block_ids,
        changed_blocks,
        changed_ranks,
    })
}

pub fn generate_inverse_ops(
    before: &ReportDocument,
    after: &ReportDocument,
) -> Result<Vec<PatchOp>, StoreError> {
    let mut ops = Vec::new();
    if before.cover != after.cover {
        ops.push(PatchOp::SetCover {
            cover: Box::new(before.cover.clone()),
        });
    }
    if before.toc != after.toc {
        ops.push(PatchOp::SetToc {
            toc: before.toc.clone(),
        });
    }
    if before.references != after.references {
        ops.push(PatchOp::SetReferences {
            references: before.references.clone(),
        });
    }
    if before.lint != after.lint {
        ops.push(PatchOp::SetLint {
            lint: before.lint.clone(),
        });
    }
    if before.theme != after.theme {
        ops.push(PatchOp::SetTheme {
            theme: Box::new(before.theme.clone()),
        });
    }
    if before.sources != after.sources {
        ops.push(PatchOp::SetSources {
            sources: before.sources.clone(),
        });
    }

    let mut simulation = after.clone();
    let before_ids: HashSet<&str> = before.blocks.iter().map(|b| b.id.as_str()).collect();
    let extras: Vec<BlockId> = simulation
        .blocks
        .iter()
        .filter(|block| !before_ids.contains(block.id.as_str()))
        .map(|block| block.id.clone())
        .collect();
    if !extras.is_empty() {
        push_inverse(
            &mut simulation,
            &mut ops,
            PatchOp::DeleteBlocks { block_ids: extras },
        )?;
    }

    let mut previous: Option<BlockId> = None;
    for (index, target) in before.blocks.iter().enumerate() {
        let existing = simulation.block(target.id.as_str()).cloned();
        match existing {
            None => {
                let op = if simulation.blocks.is_empty() {
                    PatchOp::Append {
                        blocks: vec![target.clone()],
                    }
                } else if let Some(previous) = &previous {
                    PatchOp::InsertAfter {
                        target_id: previous.clone(),
                        blocks: vec![target.clone()],
                    }
                } else {
                    PatchOp::InsertBefore {
                        target_id: simulation
                            .blocks
                            .first()
                            .map(|block| block.id.clone())
                            .ok_or_else(|| {
                                StoreError::Internal(
                                    "cannot insert before an empty simulation".into(),
                                )
                            })?,
                        blocks: vec![target.clone()],
                    }
                };
                push_inverse(&mut simulation, &mut ops, op)?;
            }
            Some(existing) if existing != *target => {
                push_inverse(
                    &mut simulation,
                    &mut ops,
                    PatchOp::ReplaceRange {
                        start_id: target.id.clone(),
                        end_id: target.id.clone(),
                        blocks: vec![target.clone()],
                    },
                )?;
            }
            Some(_) => {}
        }
        if simulation.position(target.id.as_str()) != Some(index) {
            push_inverse(
                &mut simulation,
                &mut ops,
                PatchOp::MoveBlock {
                    block_id: target.id.clone(),
                    after_id: previous.clone(),
                },
            )?;
        }
        previous = Some(target.id.clone());
    }

    if simulation.blocks != before.blocks {
        return Err(StoreError::Internal(
            "inverse operation simulation did not restore the previous block order".into(),
        ));
    }
    Ok(ops)
}

fn push_inverse(
    simulation: &mut ReportDocument,
    ops: &mut Vec<PatchOp>,
    op: PatchOp,
) -> Result<(), StoreError> {
    apply_operations_in_place(simulation, std::slice::from_ref(&op))?;
    ops.push(op);
    Ok(())
}
