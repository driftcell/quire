use super::*;

impl PgReportStore {
    pub async fn create_document(
        &self,
        document: &ReportDocument,
        actor: &str,
        idempotency_key: Option<&str>,
    ) -> Result<CreateResult, StoreError> {
        self.create_document_with_cleared_previews(document, actor, idempotency_key, Vec::new())
            .await
    }

    /// Create a document and atomically clear the previews superseded by revision 1.
    pub async fn create_document_with_cleared_previews(
        &self,
        document: &ReportDocument,
        actor: &str,
        idempotency_key: Option<&str>,
        cleared_preview_ids: Vec<String>,
    ) -> Result<CreateResult, StoreError> {
        let _guard = self.document_guard(document.doc_id.as_str()).await;
        let mut normalized = document.clone();
        let _ = normalize_document(&mut normalized);
        let document = &normalized;
        validate_doc_id(document.doc_id.as_str())?;

        validate_source_registry(&document.sources)?;
        let title = document.title();
        let cover = serde_json::to_value(&document.cover)?;
        let toc = serde_json::to_value(&document.toc)?;
        let theme = serde_json::to_value(&document.theme)?;
        let references = serde_json::to_value(&document.references)?;
        let lint = serde_json::to_value(&document.lint)?;

        let mut tx = self.pool.begin().await?;
        if let Some(key) = idempotency_key
            && let Some(existing) = sqlx::query!(
                r#"SELECT doc_id, session_id, current_rev,
                          COALESCE(NULLIF(cover->>'title',''), doc_id) AS "title!"
                   FROM report.documents WHERE idempotency_key = $1"#,
                key
            )
            .fetch_optional(&mut *tx)
            .await?
        {
            tx.commit().await?;
            let doc_id = DocId::parse(existing.doc_id)?;
            let revision = Revision::new(existing.current_rev)?;
            let session_id = existing.session_id.map(SessionId::parse).transpose()?;
            return Ok(CreateResult {
                doc_id,
                revision,
                title: existing.title,
                session_id,
                idempotent_replay: true,
            });
        }

        let inserted = sqlx::query!(
            r#"INSERT INTO report.documents
               (doc_id, session_id, idempotency_key, current_rev, cover, toc, theme, "references", lint, target_prose_chars)
               VALUES ($1, $2, $3, 1, $4, $5, $6, $7, $8, $9)
               RETURNING current_rev"#,
            document.doc_id.as_str(),
            document.session_id.as_ref().map(SessionId::as_str),
            idempotency_key,
            cover,
            toc,
            theme,
            references,
            lint,
            document.target_prose_chars.map(|n| i64::from(n.get())),
        )
        .fetch_one(&mut *tx)
        .await;

        let current_rev = match inserted {
            Ok(row) => Revision::new(row.current_rev)?,
            Err(error) => {
                if is_unique_violation(&error) {
                    let _ = tx.rollback().await;
                    if let Some(key) = idempotency_key
                        && let Some(existing) = sqlx::query!(
                            r#"SELECT doc_id, session_id, current_rev,
                                      COALESCE(NULLIF(cover->>'title',''), doc_id) AS "title!"
                               FROM report.documents WHERE idempotency_key = $1"#,
                            key
                        )
                        .fetch_optional(&self.pool)
                        .await?
                    {
                        let existing_doc_id = DocId::parse(existing.doc_id)?;
                        let existing_revision = Revision::new(existing.current_rev)?;
                        let existing_session =
                            existing.session_id.map(SessionId::parse).transpose()?;
                        return Ok(CreateResult {
                            doc_id: existing_doc_id,
                            revision: existing_revision,
                            title: existing.title,
                            session_id: existing_session,
                            idempotent_replay: true,
                        });
                    }
                    return Err(StoreError::AlreadyExists {
                        doc_id: document.doc_id.clone(),
                    });
                }
                return Err(error.into());
            }
        };

        for (source_id, source) in &document.sources {
            validate_source_id(source_id)?;
            sqlx::query!(
                r#"INSERT INTO report.sources (doc_id, source_id, normalized_key, metadata, excerpt)
                   VALUES ($1, $2, $3, $4, $5)"#,
                document.doc_id.as_str(),
                source_id.as_str(),
                normalized_source_key(source),
                serde_json::to_value(source)?,
                source.excerpt.as_deref()
            )
            .execute(&mut *tx)
            .await?;
        }

        let ranks = quire_protocol::rank::ranks_for_count(document.blocks.len());
        for (block, rank) in document.blocks.iter().zip(ranks.iter()) {
            insert_block_row(&mut tx, document.doc_id.as_str(), block, rank, current_rev).await?;
        }

        let mut initial_ops = vec![
            PatchOp::SetCover {
                cover: Box::new(document.cover.clone()),
            },
            PatchOp::SetToc {
                toc: document.toc.clone(),
            },
            PatchOp::SetReferences {
                references: document.references.clone(),
            },
            PatchOp::SetLint {
                lint: document.lint.clone(),
            },
            PatchOp::SetTheme {
                theme: Box::new(document.theme.clone()),
            },
            PatchOp::SetSources {
                sources: document.sources.clone(),
            },
        ];
        if !document.blocks.is_empty() {
            initial_ops.push(PatchOp::Append {
                blocks: document.blocks.clone(),
            });
        }
        let forward_ops = ops_to_json(&initial_ops)?;
        let initial_block_ids: Vec<String> = document
            .blocks
            .iter()
            .map(|block| block.id.as_str().to_string())
            .collect();
        sqlx::query!(
            r#"INSERT INTO report.revisions
               (doc_id, rev_num, actor, summary, forward_ops, inverse_ops, affected_block_ids)
               VALUES ($1, $2, $3, $4, $5, '[]'::jsonb, $6)"#,
            document.doc_id.as_str(),
            current_rev.get(),
            actor,
            "create_report",
            forward_ops,
            &initial_block_ids
        )
        .execute(&mut *tx)
        .await?;

        tx.commit().await?;
        let event = RevisionEvent::new(
            document.doc_id.clone(),
            current_rev,
            actor,
            "create_report",
            forward_ops,
        )
        .with_cleared_preview_ids(cleared_preview_ids);
        if let Err(error) = self.bus.publish_revision(&event).await {
            warn!(%error, doc_id = %document.doc_id, "failed to publish create revision");
        }
        if let Some(session_id) = &document.session_id {
            let session_event = SessionEvent {
                event: "doc_created".into(),
                session_id: session_id.clone(),
                doc_id: document.doc_id.clone(),
                title: title.clone(),
                revision: current_rev,
                created_at: Utc::now(),
            };
            if let Err(error) = self.bus.publish_session_event(&session_event).await {
                warn!(%error, session_id = session_id.as_str(), "failed to publish session doc_created event");
            }
        }

        Ok(CreateResult {
            doc_id: document.doc_id.clone(),
            revision: current_rev,
            title,
            session_id: document.session_id.clone(),
            idempotent_replay: false,
        })
    }

    /// Load a document and its ranks in one repeatable-read transaction.
    pub async fn load_document(&self, doc_id: &DocId) -> Result<LoadedDocument, StoreError> {
        let mut tx = self.pool.begin().await?;
        sqlx::query("SET TRANSACTION ISOLATION LEVEL REPEATABLE READ READ ONLY")
            .execute(&mut *tx)
            .await?;
        let loaded = load_document_in_tx(&mut tx, doc_id.as_str()).await?;
        tx.commit().await?;
        Ok(loaded)
    }

    pub async fn get_document(&self, doc_id: &DocId) -> Result<ReportDocument, StoreError> {
        Ok(self.load_document(doc_id).await?.document)
    }

    pub async fn snapshot(&self, doc_id: &DocId) -> Result<DocumentSnapshot, StoreError> {
        let loaded = self.load_document(doc_id).await?;
        Ok(DocumentSnapshot {
            current_rev: loaded.document.revision,
            document: loaded.document,
        })
    }

    pub async fn get_document_session_id(
        &self,
        doc_id: &DocId,
    ) -> Result<Option<SessionId>, StoreError> {
        let row = sqlx::query!(
            r#"SELECT session_id FROM report.documents WHERE doc_id = $1"#,
            doc_id.as_str()
        )
        .fetch_optional(&self.pool)
        .await?;
        let Some(row) = row else {
            return Err(StoreError::NotFound {
                doc_id: doc_id.clone(),
            });
        };
        row.session_id
            .map(SessionId::parse)
            .transpose()
            .map_err(StoreError::from)
    }

    pub async fn list_reports_for_scope(
        &self,
        query: Option<&str>,
        limit: i64,
        offset: i64,
        allowed_sessions: &[String],
        allowed_docs: &[String],
    ) -> Result<ReportList, StoreError> {
        let limit = limit.clamp(1, 100);
        let offset = offset.max(0);
        let query = query
            .map(str::trim)
            .filter(|value| !value.is_empty())
            .map(|value| format!("%{value}%"));
        let rows = sqlx::query!(
            r#"
            SELECT d.doc_id,
                   COALESCE(NULLIF(d.cover->>'title',''), d.doc_id) AS "title!",
                   d.session_id,
                   d.current_rev,
                   (SELECT COUNT(*) FROM report.blocks b WHERE b.doc_id = d.doc_id) AS "block_count!",
                   d.updated_at
            FROM report.documents d
            WHERE ($1::text IS NULL
                   OR d.doc_id ILIKE $1
                   OR COALESCE(d.cover->>'title','') ILIKE $1)
              AND (d.session_id = ANY($2) OR d.doc_id = ANY($3))
            ORDER BY d.updated_at DESC, d.doc_id ASC
            LIMIT $4 OFFSET $5
            "#,
            query.as_deref(),
            allowed_sessions,
            allowed_docs,
            limit,
            offset,
        )
        .fetch_all(&self.pool)
        .await?;
        let total = sqlx::query!(
            r#"
            SELECT COUNT(*) AS "count!"
            FROM report.documents d
            WHERE ($1::text IS NULL
                   OR d.doc_id ILIKE $1
                   OR COALESCE(d.cover->>'title','') ILIKE $1)
              AND (d.session_id = ANY($2) OR d.doc_id = ANY($3))
            "#,
            query.as_deref(),
            allowed_sessions,
            allowed_docs,
        )
        .fetch_one(&self.pool)
        .await?
        .count;
        Ok(ReportList {
            reports: rows
                .into_iter()
                .map(|row| {
                    Ok(ReportSummary {
                        doc_id: DocId::parse(row.doc_id)?,
                        title: row.title,
                        session_id: row.session_id.map(SessionId::parse).transpose()?,
                        current_rev: Revision::new(row.current_rev)?,
                        block_count: row.block_count,
                        updated_at: row.updated_at,
                    })
                })
                .collect::<Result<Vec<_>, StoreError>>()?,
            total,
            limit,
            offset,
        })
    }

    pub async fn list_reports(
        &self,
        query: Option<&str>,
        limit: i64,
        offset: i64,
    ) -> Result<ReportList, StoreError> {
        let limit = limit.clamp(1, 100);
        let offset = offset.max(0);
        let query = query
            .map(str::trim)
            .filter(|value| !value.is_empty())
            .map(|value| format!("%{value}%"));
        let rows = sqlx::query!(
            r#"
            SELECT d.doc_id,
                   COALESCE(NULLIF(d.cover->>'title',''), d.doc_id) AS "title!",
                   d.session_id,
                   d.current_rev,
                   (SELECT COUNT(*) FROM report.blocks b WHERE b.doc_id = d.doc_id) AS "block_count!",
                   d.updated_at
            FROM report.documents d
            WHERE ($1::text IS NULL
                   OR d.doc_id ILIKE $1
                   OR COALESCE(d.cover->>'title','') ILIKE $1)
            ORDER BY d.updated_at DESC, d.doc_id ASC
            LIMIT $2 OFFSET $3
            "#,
            query.as_deref(),
            limit,
            offset,
        )
        .fetch_all(&self.pool)
        .await?;
        let total = sqlx::query!(
            r#"
            SELECT COUNT(*) AS "count!"
            FROM report.documents d
            WHERE ($1::text IS NULL
                   OR d.doc_id ILIKE $1
                   OR COALESCE(d.cover->>'title','') ILIKE $1)
            "#,
            query.as_deref(),
        )
        .fetch_one(&self.pool)
        .await?
        .count;
        Ok(ReportList {
            reports: rows
                .into_iter()
                .map(|row| {
                    Ok(ReportSummary {
                        doc_id: DocId::parse(row.doc_id)?,
                        title: row.title,
                        session_id: row.session_id.map(SessionId::parse).transpose()?,
                        current_rev: Revision::new(row.current_rev)?,
                        block_count: row.block_count,
                        updated_at: row.updated_at,
                    })
                })
                .collect::<Result<Vec<_>, StoreError>>()?,
            total,
            limit,
            offset,
        })
    }

    pub async fn list_revisions(
        &self,
        doc_id: &DocId,
        limit: i64,
    ) -> Result<Vec<RevisionRecord>, StoreError> {
        let limit = limit.clamp(1, 500);
        let rows = sqlx::query!(
            r#"SELECT rev_num, actor, summary, forward_ops, inverse_ops, affected_block_ids, created_at
               FROM report.revisions WHERE doc_id = $1
               ORDER BY rev_num DESC LIMIT $2"#,
            doc_id.as_str(),
            limit
        )
        .fetch_all(&self.pool)
        .await?;
        rows.into_iter()
            .map(|row| {
                Ok(RevisionRecord {
                    doc_id: doc_id.clone(),
                    revision: Revision::new(row.rev_num)?,
                    actor: row.actor,
                    summary: row.summary,
                    forward_ops: row.forward_ops,
                    inverse_ops: row.inverse_ops,
                    affected_block_ids: row
                        .affected_block_ids
                        .iter()
                        .map(BlockId::parse)
                        .collect::<Result<Vec<_>, _>>()?,
                    created_at: row.created_at,
                })
            })
            .collect::<Result<Vec<_>, StoreError>>()
    }

    pub async fn revision_events_since_pg(
        &self,
        doc_id: &DocId,
        since_rev: Revision,
        limit: i64,
    ) -> Result<Vec<RevisionEvent>, StoreError> {
        let rows = sqlx::query!(
            r#"SELECT rev_num, actor, summary, forward_ops, created_at
               FROM report.revisions
               WHERE doc_id = $1 AND rev_num > $2
               ORDER BY rev_num ASC LIMIT $3"#,
            doc_id.as_str(),
            since_rev.get(),
            limit.clamp(1, 5000)
        )
        .fetch_all(&self.pool)
        .await?;
        rows.into_iter()
            .map(|row| {
                Ok(RevisionEvent {
                    doc_id: doc_id.clone(),
                    revision: Revision::new(row.rev_num)?,
                    actor: row.actor,
                    summary: row.summary,
                    ops: row.forward_ops,
                    cleared_preview_ids: Vec::new(),
                    created_at: row.created_at,
                })
            })
            .collect::<Result<Vec<_>, StoreError>>()
    }
}

pub(crate) async fn lock_current_revision(
    tx: &mut Transaction<'_, Postgres>,
    doc_id: &str,
) -> Result<Revision, StoreError> {
    let typed_doc_id = DocId::parse(doc_id)?;
    let row = sqlx::query!(
        "SELECT current_rev FROM report.documents WHERE doc_id = $1 FOR UPDATE",
        doc_id
    )
    .fetch_optional(&mut **tx)
    .await?
    .ok_or_else(|| StoreError::NotFound {
        doc_id: typed_doc_id,
    })?;
    Ok(Revision::new(row.current_rev)?)
}

pub(crate) async fn load_document_in_tx(
    tx: &mut Transaction<'_, Postgres>,
    doc_id: &str,
) -> Result<LoadedDocument, StoreError> {
    let typed_doc_id = DocId::parse(doc_id)?;
    let row = sqlx::query!(
        r#"SELECT doc_id, session_id, current_rev, cover, toc, theme, target_prose_chars,
                  "references" AS "references_json!", lint AS "lint_json!"
           FROM report.documents WHERE doc_id = $1"#,
        doc_id
    )
    .fetch_optional(&mut **tx)
    .await?
    .ok_or_else(|| StoreError::NotFound {
        doc_id: typed_doc_id,
    })?;
    let doc_row = DocRow {
        doc_id: DocId::parse(row.doc_id)?,
        session_id: row.session_id.map(SessionId::parse).transpose()?,
        current_rev: Revision::new(row.current_rev)?,
        cover: row.cover,
        toc: row.toc,
        theme: row.theme,
        references: row.references_json,
        lint: row.lint_json,
    };

    let source_rows = sqlx::query!(
        "SELECT source_id, metadata, excerpt FROM report.sources WHERE doc_id = $1 ORDER BY source_id",
        doc_id
    )
    .fetch_all(&mut **tx)
    .await?;
    let mut sources = IndexMap::new();
    for row in source_rows {
        let mut entry: SourceEntry = parse_json(row.metadata, "report.sources.metadata")?;
        if entry.excerpt.is_none() {
            entry.excerpt = row.excerpt;
        }
        sources.insert(SourceId::parse(row.source_id)?, entry);
    }

    let block_rows = sqlx::query!(
        r#"SELECT block_id, rank, payload FROM report.blocks
           WHERE doc_id = $1 ORDER BY rank COLLATE "C""#,
        doc_id
    )
    .fetch_all(&mut **tx)
    .await?;
    let mut blocks = Vec::with_capacity(block_rows.len());
    let mut ranks = BTreeMap::new();
    for row in block_rows {
        let block: Block = parse_json(row.payload, "report.blocks.payload")?;
        if block.id != row.block_id {
            return Err(StoreError::Internal(format!(
                "block payload id `{}` does not match row id `{}`",
                block.id, row.block_id
            )));
        }
        ranks.insert(BlockId::parse(row.block_id)?, Rank::parse(row.rank)?);
        blocks.push(block);
    }

    let document = ReportDocument {
        doc_id: doc_row.doc_id,
        session_id: doc_row.session_id,
        target_prose_chars: row
            .target_prose_chars
            .map(|value| {
                u32::try_from(value)
                    .ok()
                    .and_then(std::num::NonZeroU32::new)
                    .ok_or_else(|| StoreError::Internal("invalid stored target_prose_chars".into()))
            })
            .transpose()?,
        revision: doc_row.current_rev,
        cover: parse_json(doc_row.cover, "report.documents.cover")?,
        toc: parse_json(doc_row.toc, "report.documents.toc")?,
        theme: parse_json(doc_row.theme, "report.documents.theme")?,
        sources,
        blocks,
        references: parse_json(doc_row.references, "report.documents.references")?,
        lint: parse_json(doc_row.lint, "report.documents.lint")?,
    };
    Ok(LoadedDocument { document, ranks })
}
