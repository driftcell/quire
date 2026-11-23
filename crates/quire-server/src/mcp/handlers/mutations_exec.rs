use super::*;

impl QuireMcpServer {
    pub async fn exec_apply_operations(
        &self,
        context: RequestContext,
        request: ApplyOperationsParams,
    ) -> Result<Json<ApplyOperationsReceipt>, ErrorData> {
        if request.summary.trim().is_empty() {
            return Err(invalid_params(
                "summary must not be empty",
                "Describe the intent, for example `write chapter 2 executive summary`.",
                None,
            ));
        }
        let actor = context.actor;
        let doc_id = parse_doc_id(&request.doc_id)?;
        let expected_revision = parse_revision(request.expected_revision)?;
        let source_upserts = inline_source_upserts(request.sources)?;
        let preview_ids = normalize_preview_ids(request.preview_ids)?;
        let result = self
            .state
            .store
            .apply_operations_with_extras(
                &doc_id,
                expected_revision,
                &actor,
                request.summary.trim(),
                &request.ops,
                CommitExtras {
                    sources: source_upserts,
                    cleared_preview_ids: preview_ids.clone(),
                },
            )
            .await
            .map_err(mcp_error)?;
        if result.outcome.no_op {
            publish_ephemeral_clear(&self.state, &doc_id, &preview_ids).await;
        }
        let touched = touched_blocks(&request.ops, &result.outcome.assigned_ids);
        let suppress_empty_heading = is_heading_only_batch(&request.ops);
        let suppress_consecutive_visuals = is_visual_only_batch(&request.ops);
        let (blocking_errors, inline_warnings) = split_scoped_diagnostics(
            result.inline_warnings,
            &touched,
            suppress_empty_heading,
            suppress_consecutive_visuals,
        );
        let _next_hint = lint_hint::apply_operations_next_hint(
            result.new_revision.get(),
            &blocking_errors,
            &inline_warnings,
            result.outcome.no_op,
            result.rebased,
            result.rebased_over,
            result.base_revision.get(),
        );
        Ok(Json(ApplyOperationsReceipt {
            new_revision: result.new_revision.get(),
            base_revision: result.base_revision.get(),
            rebased: result.rebased,
            rebased_over: result.rebased_over,
            committed: true,
            assigned_ids: result.outcome.assigned_ids,
            inline_warnings,
            blocking_errors,
            _next_hint,
        }))
    }

    pub async fn exec_rollback_revision(
        &self,
        context: RequestContext,
        request: RollbackParams,
    ) -> Result<Json<RollbackReceipt>, ErrorData> {
        let actor = context.actor;
        let doc_id = parse_doc_id(&request.doc_id)?;
        let target_revision = parse_revision(request.target_revision)?;
        let result = self
            .state
            .store
            .rollback_to_revision(&doc_id, target_revision, &actor)
            .await
            .map_err(mcp_error)?;
        let summary = format!(
            "Rolled back to revision {} by replaying {} inverse revision(s); history is preserved as revision {}.",
            result.target_revision, result.replayed_revisions, result.new_revision
        );
        let _next_hint = format!(
            "Call get_document_digest or read_blocks to inspect revision {}, then lint_document before continuing edits.",
            result.new_revision
        );
        Ok(Json(RollbackReceipt {
            doc_id: doc_id.to_string(),
            new_revision: result.new_revision.get(),
            rolled_back_to: result.target_revision.get(),
            replayed_revisions: result.replayed_revisions,
            summary,
            _next_hint,
        }))
    }

    pub async fn exec_upsert_sources(
        &self,
        context: RequestContext,
        request: UpsertSourcesParams,
    ) -> Result<Json<UpsertSourcesReceipt>, ErrorData> {
        if request.sources.is_empty() {
            return Err(invalid_params(
                "sources must not be empty",
                "Supply at least one source object with id/title (and preferably url).",
                None,
            ));
        }
        let actor = context.actor;
        let doc_id = parse_doc_id(&request.doc_id)?;
        let upserts: Vec<SourceUpsert> = request
            .sources
            .into_iter()
            .map(|source| {
                let source_id = source.id.ok_or_else(|| {
                    invalid_params(
                        "source id is required",
                        "Every upsert_sources entry needs `id`, e.g. `\"id\":\"sec10\"`.",
                        None,
                    )
                })?;
                Ok(SourceUpsert {
                    source_id,
                    source: source.entry,
                })
            })
            .collect::<Result<Vec<_>, ErrorData>>()?;
        let result = self
            .state
            .store
            .upsert_sources(&doc_id, &actor, upserts)
            .await
            .map_err(mcp_error)?;
        // Suppress transient `QR102` (unused-source) and block-scoped layout/structure warnings
        // inside `upsert_sources` receipts: newly registered sources are naturally uncited until
        // subsequent `apply_operations` commits, and full-document `lint_document` /
        // `configure_references` verify orphan sources at verification time.
        let inline_warnings = result
            .inline_warnings
            .into_iter()
            .filter(|diagnostic| diagnostic.code != "QR102" && diagnostic.block_id.is_none())
            .collect();
        let _next_hint = if result.changed {
            format!(
                "Sources committed at revision {}. Use canonical_id_map values in [^source_id] markers.",
                result.revision
            )
        } else {
            "Sources already existed; no revision was created. Use canonical_id_map when citing."
                .to_string()
        };
        Ok(Json(UpsertSourcesReceipt {
            doc_id: doc_id.to_string(),
            revision: result.revision.get(),
            changed: result.changed,
            canonical_id_map: result
                .canonical_id_map
                .into_iter()
                .map(|(from, to)| (from.to_string(), to.to_string()))
                .collect(),
            source_count: result.sources.len(),
            inline_warnings,
            _next_hint,
        }))
    }

    pub(crate) async fn apply_with_reload<F>(
        &self,
        doc_id: &DocId,
        actor: &str,
        summary: &str,
        mut build_ops: F,
    ) -> Result<(ApplyResult, ReportDocument), ErrorData>
    where
        F: FnMut(&ReportDocument) -> Result<Vec<PatchOp>, ErrorData>,
    {
        for attempt in 0..4 {
            let document = self
                .state
                .store
                .get_document(doc_id)
                .await
                .map_err(mcp_error)?;
            let ops = build_ops(&document)?;
            match self
                .state
                .store
                .apply_operations(doc_id, document.revision, actor, summary, &ops)
                .await
            {
                Ok(result) => {
                    let updated = self
                        .state
                        .store
                        .get_document(doc_id)
                        .await
                        .map_err(mcp_error)?;
                    return Ok((result, updated));
                }
                Err(StoreError::RevisionAhead { .. } | StoreError::Conflict(_)) if attempt < 3 => {
                    tokio::task::yield_now().await;
                    continue;
                }
                Err(error) => return Err(mcp_error(error)),
            }
        }
        Err(invalid_params(
            "configuration update could not be rebased after 4 attempts",
            "Reload the document and retry set_cover/configure_*.",
            None,
        ))
    }
}

fn touched_blocks(ops: &[PatchOp], assigned: &[IdAssignment]) -> BTreeSet<BlockId> {
    let mut touched: BTreeSet<BlockId> = ops.iter().flat_map(PatchOp::touched_block_ids).collect();
    touched.extend(
        assigned
            .iter()
            .map(|assignment| assignment.assigned.clone()),
    );
    touched
}

/// Returns `true` when every operation in `ops` only creates or patches `Heading` blocks (e.g.
/// Step 2 skeleton creation or heading title upgrades), where newly created headings naturally
/// have no body paragraphs yet until downstream `analyst` and `writer` subagents run.
fn is_heading_only_batch(ops: &[PatchOp]) -> bool {
    !ops.is_empty()
        && ops.iter().all(|op| match op {
            PatchOp::InsertAfter { blocks, .. }
            | PatchOp::InsertBefore { blocks, .. }
            | PatchOp::Append { blocks }
            | PatchOp::AppendToSection { blocks, .. } => {
                !blocks.is_empty()
                    && blocks
                        .iter()
                        .all(|block| matches!(&block.body, BlockKind::Heading { .. }))
            }
            PatchOp::PatchBlock { patch, .. } => {
                (patch.text.is_some() || patch.level.is_some())
                    && patch.title.is_none()
                    && patch.note.is_none()
                    && patch.option.is_none()
                    && patch.option_patch.is_none()
                    && patch.columns.is_none()
                    && patch.rows.is_none()
                    && patch.summary_rows.is_none()
                    && patch.append_rows.is_empty()
                    && patch.update_cell.is_none()
            }
            _ => false,
        })
}

/// Returns `true` when every operation in `ops` only creates or patches visual blocks (`Table`,
/// `Echarts`, `CustomHtml`), such as Step 4 when `analyst` subagents commit visual blocks into the
/// heading skeleton before `writer` subagents insert intervening analytical prose in Step 5.
fn is_visual_only_batch(ops: &[PatchOp]) -> bool {
    !ops.is_empty()
        && ops.iter().all(|op| match op {
            PatchOp::InsertAfter { blocks, .. }
            | PatchOp::InsertBefore { blocks, .. }
            | PatchOp::Append { blocks }
            | PatchOp::AppendToSection { blocks, .. } => {
                !blocks.is_empty()
                    && blocks.iter().all(|block| {
                        matches!(
                            &block.body,
                            BlockKind::Table(_) | BlockKind::Echarts(_) | BlockKind::CustomHtml(_)
                        )
                    })
            }
            PatchOp::PatchBlock { patch, .. } => {
                patch.text.is_none()
                    && patch.level.is_none()
                    && (patch.title.is_some()
                        || patch.note.is_some()
                        || patch.option.is_some()
                        || patch.option_patch.is_some()
                        || patch.columns.is_some()
                        || patch.rows.is_some()
                        || patch.summary_rows.is_some()
                        || !patch.append_rows.is_empty()
                        || patch.update_cell.is_some()
                        || patch.height.is_some()
                        || patch.fallback_summary.is_some()
                        || patch.fallback_text.is_some()
                        || patch.lint_ignore.is_some()
                        || patch.style.is_some()
                        || patch.source_ids.is_some()
                        || patch.source_note.is_some())
            }
            _ => false,
        })
}

fn split_scoped_diagnostics(
    diagnostics: Vec<Diagnostic>,
    touched: &BTreeSet<BlockId>,
    suppress_empty_heading: bool,
    suppress_consecutive_visuals: bool,
) -> (Vec<Diagnostic>, Vec<Diagnostic>) {
    let scoped: Vec<Diagnostic> = if touched.is_empty() {
        diagnostics
    } else {
        diagnostics
            .into_iter()
            .filter(|diagnostic| {
                if suppress_empty_heading && diagnostic.code == "QR403" {
                    return false;
                }
                if suppress_consecutive_visuals && diagnostic.code == "QR416" {
                    return false;
                }
                diagnostic
                    .block_id
                    .as_ref()
                    .is_some_and(|block_id| touched.contains(block_id))
            })
            .collect()
    };
    scoped
        .into_iter()
        .partition(|diagnostic| diagnostic.severity == Severity::Error)
}
