//! PostgreSQL row-per-block persistence, Smart CAS, revisions and snapshots.

use std::{
    collections::{BTreeMap, BTreeSet, HashMap, HashSet},
    sync::{Arc, Mutex as StdMutex},
};

use chrono::{DateTime, Utc};
use indexmap::IndexMap;
use quire_protocol::{
    Block, BlockId, BlockKind, CommentAuthorType, CommentMessage, CommentStatus, CommentThread,
    DocId, PatchOp, PatchOutcome, Rank, ReportDocument, Revision, SessionId, SourceEntry, SourceId,
    apply_operations_in_place, lint_document, normalize::normalize_document, rank_after,
    remap_operations_sources, try_rank_between,
};
use serde::de::DeserializeOwned;
use serde_json::Value;
use sqlx::{PgPool, Postgres, Transaction};
use tokio::sync::{Mutex as AsyncMutex, OwnedMutexGuard};
use tracing::{error, warn};
use uuid::Uuid;

use crate::{
    error::{InlineWarnings, RevisionConflict, StoreError, ops_to_json},
    redis_stream::RedisStreamBus,
    types::{
        ApplyResult, CommitExtras, CreateResult, DocumentDiff, DocumentSnapshot, NewCommentReply,
        NewCommentThread, ReportList, ReportSummary, RevisionEvent, RevisionRecord, RollbackResult,
        SessionEvent, SourceUpsert, UpsertSourcesResult,
    },
};

#[derive(Debug, Clone)]
pub struct LoadedDocument {
    pub document: ReportDocument,
    pub ranks: BTreeMap<BlockId, Rank>,
}

#[derive(Debug, Clone)]
struct DocRow {
    doc_id: DocId,
    session_id: Option<SessionId>,
    current_rev: Revision,
    cover: Value,
    toc: Value,
    theme: Value,
    references: Value,
}

#[derive(Debug, Clone)]
struct CommentThreadRow {
    doc_id: String,
    thread_id: String,
    block_id: String,
    section_id: Option<String>,
    anchor_snapshot: Option<String>,
    quote: Option<Value>,
    status: String,
    created_rev: i64,
    addressed_rev: Option<i64>,
    created_at: DateTime<Utc>,
    updated_at: DateTime<Utc>,
    anchor_detached: bool,
}

#[derive(Clone)]
pub struct PgReportStore {
    pool: PgPool,
    bus: RedisStreamBus,
    /// Serializes commit + Redis publication per document so stream entries arrive in exact
    /// revision order. PostgreSQL row locks alone release before XADD.
    document_locks: Arc<StdMutex<HashMap<String, Arc<AsyncMutex<()>>>>>,
}

impl PgReportStore {
    pub async fn connect(database_url: &str, redis_url: &str) -> Result<Self, StoreError> {
        let pool = sqlx::postgres::PgPoolOptions::new()
            .max_connections(20)
            .acquire_timeout(std::time::Duration::from_secs(10))
            .connect(database_url)
            .await?;
        let bus = RedisStreamBus::connect(redis_url).await?;
        Ok(Self {
            pool,
            bus,
            document_locks: Arc::new(StdMutex::new(HashMap::new())),
        })
    }

    pub fn from_pool(pool: PgPool, bus: RedisStreamBus) -> Self {
        Self {
            pool,
            bus,
            document_locks: Arc::new(StdMutex::new(HashMap::new())),
        }
    }

    async fn document_guard(&self, doc_id: &str) -> OwnedMutexGuard<()> {
        let lock = {
            let mut locks = self
                .document_locks
                .lock()
                .unwrap_or_else(|poisoned| poisoned.into_inner());
            locks
                .entry(doc_id.to_string())
                .or_insert_with(|| Arc::new(AsyncMutex::new(())))
                .clone()
        };
        lock.lock_owned().await
    }

    pub fn pool(&self) -> &PgPool {
        &self.pool
    }

    pub fn bus(&self) -> &RedisStreamBus {
        &self.bus
    }

    pub async fn health(&self) -> Result<(), StoreError> {
        sqlx::query_scalar::<_, i64>("SELECT 1::bigint")
            .fetch_one(&self.pool)
            .await?;
        self.bus.health().await
    }

    /// Apply embedded migrations. Useful for the server binary and E2E runs.
    pub async fn migrate(&self) -> Result<(), StoreError> {
        sqlx::migrate!("./migrations")
            .run(&self.pool)
            .await
            .map_err(|error| StoreError::Internal(format!("migration failed: {error}")))
    }

    /// Create a document at revision 1. Idempotent when `idempotency_key` is supplied.
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

        let mut tx = self.pool.begin().await?;
        if let Some(key) = idempotency_key
            && let Some(existing) = sqlx::query!(
                r#"SELECT doc_id, session_id, current_rev,
                          COALESCE(NULLIF(cover->>'title',''), doc_id) AS "title!"
                   FROM report_documents WHERE idempotency_key = $1"#,
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
            r#"INSERT INTO report_documents
               (doc_id, session_id, idempotency_key, current_rev, cover, toc, theme, "references")
               VALUES ($1, $2, $3, 1, $4, $5, $6, $7)
               RETURNING current_rev"#,
            document.doc_id.as_str(),
            document.session_id.as_ref().map(SessionId::as_str),
            idempotency_key,
            cover,
            toc,
            theme,
            references,
        )
        .fetch_one(&mut *tx)
        .await;

        let current_rev = match inserted {
            Ok(row) => Revision::new(row.current_rev)?,
            Err(error) => {
                if is_unique_violation(&error) {
                    if let Some(key) = idempotency_key
                        && let Some(existing) = sqlx::query!(
                            r#"SELECT doc_id, session_id, current_rev,
                                      COALESCE(NULLIF(cover->>'title',''), doc_id) AS "title!"
                               FROM report_documents WHERE idempotency_key = $1"#,
                            key
                        )
                        .fetch_optional(&mut *tx)
                        .await?
                    {
                        tx.commit().await?;
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
                r#"INSERT INTO report_sources (doc_id, source_id, normalized_key, metadata, excerpt)
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
                cover: document.cover.clone(),
            },
            PatchOp::SetToc {
                toc: document.toc.clone(),
            },
            PatchOp::SetReferences {
                references: document.references.clone(),
            },
            PatchOp::SetTheme {
                theme: document.theme.clone(),
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
            r#"INSERT INTO report_revisions
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
                   (SELECT COUNT(*) FROM report_blocks b WHERE b.doc_id = d.doc_id) AS "block_count!",
                   d.updated_at
            FROM report_documents d
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
            FROM report_documents d
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
               FROM report_revisions WHERE doc_id = $1
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
               FROM report_revisions
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

    /// Apply patch operations transactionally with Smart CAS rebasing and inverse ops.
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

    /// Upsert sources with URL/title normalized-key deduplication. Creates one revision if changed.
    pub async fn upsert_sources(
        &self,
        doc_id: &DocId,
        actor: &str,
        upserts: Vec<SourceUpsert>,
    ) -> Result<UpsertSourcesResult, StoreError> {
        let _guard = self.document_guard(doc_id.as_str()).await;
        let mut tx = self.pool.begin().await?;
        let current_rev = lock_current_revision(&mut tx, doc_id.as_str()).await?;
        let loaded = load_document_in_tx(&mut tx, doc_id.as_str()).await?;
        let before = loaded.document.clone();

        let existing_rows = sqlx::query!(
            r#"SELECT source_id, normalized_key, metadata
               FROM report_sources WHERE doc_id = $1
               ORDER BY source_id ASC
               FOR UPDATE"#,
            doc_id.as_str()
        )
        .fetch_all(&mut *tx)
        .await?;
        let mut key_to_id: BTreeMap<String, SourceId> = existing_rows
            .iter()
            .map(|row| {
                Ok((
                    row.normalized_key.clone(),
                    SourceId::parse(row.source_id.as_str())?,
                ))
            })
            .collect::<Result<BTreeMap<_, _>, StoreError>>()?;
        let mut sources: IndexMap<SourceId, SourceEntry> = before.sources.clone();
        let mut canonical_id_map = BTreeMap::new();
        let mut db_updates: BTreeMap<SourceId, (String, SourceEntry)> = BTreeMap::new();

        for upsert in upserts {
            let normalized = normalized_source_key(&upsert.source);
            let canonical_id = if let Some(existing_id) = key_to_id.get(&normalized) {
                existing_id.clone()
            } else if sources.contains_key(&upsert.source_id) {
                // The source ID exists under another normalized key; replace its metadata/key.
                for (key, id) in key_to_id.iter() {
                    if id == &upsert.source_id {
                        let stale_key = key.clone();
                        key_to_id.remove(&stale_key);
                        break;
                    }
                }
                upsert.source_id.clone()
            } else {
                upsert.source_id.clone()
            };
            canonical_id_map.insert(upsert.source_id.clone(), canonical_id.clone());
            if canonical_id != upsert.source_id {
                // Do not create an alias; callers should use the canonical ID.
                if let Some(existing) = sources.get(&canonical_id).cloned() {
                    let merged = merge_source(&existing, &upsert.source);
                    sources.insert(canonical_id.clone(), merged.clone());
                    db_updates.insert(canonical_id.clone(), (normalized.clone(), merged));
                }
                key_to_id.insert(normalized, canonical_id);
                continue;
            }

            if let Some(existing_source) = sources.get(&upsert.source_id) {
                let old_key = normalized_source_key(existing_source);
                key_to_id.remove(&old_key);
            }
            let merged = sources
                .get(&upsert.source_id)
                .map(|existing| merge_source(existing, &upsert.source))
                .unwrap_or_else(|| upsert.source.clone());
            let final_key = normalized_source_key(&merged);
            key_to_id.insert(final_key.clone(), canonical_id.clone());
            sources.insert(canonical_id.clone(), merged.clone());
            db_updates.insert(canonical_id, (final_key, merged));
        }

        let mut after = before.clone();
        after.sources = sources;
        after.revision = current_rev;
        if after.sources == before.sources {
            tx.commit().await?;
            let warnings = inline_warnings(&after);
            return Ok(UpsertSourcesResult {
                canonical_id_map,
                revision: current_rev,
                changed: false,
                sources: after.sources,
                inline_warnings: warnings,
            });
        }

        let new_revision = current_rev.next();
        after.revision = new_revision;
        let forward = vec![PatchOp::SetSources {
            sources: after.sources.clone(),
        }];
        let inverse = vec![PatchOp::SetSources {
            sources: before.sources.clone(),
        }];
        let forward_json = ops_to_json(&forward)?;
        let inverse_json = ops_to_json(&inverse)?;
        // Validate that normalized keys remain unique.
        validate_source_registry(&after.sources)?;
        for (source_id, (normalized, source)) in &db_updates {
            sqlx::query!(
                r#"INSERT INTO report_sources (doc_id, source_id, normalized_key, metadata, excerpt)
                   VALUES ($1, $2, $3, $4, $5)
                   ON CONFLICT (doc_id, source_id)
                   DO UPDATE SET normalized_key = EXCLUDED.normalized_key,
                                 metadata = EXCLUDED.metadata,
                                 excerpt = EXCLUDED.excerpt"#,
                doc_id.as_str(),
                source_id.as_str(),
                normalized,
                serde_json::to_value(source)?,
                source.excerpt.as_deref()
            )
            .execute(&mut *tx)
            .await?;
        }
        update_document_row(&mut tx, doc_id.as_str(), &after, new_revision).await?;
        sqlx::query!(
            r#"INSERT INTO report_revisions
               (doc_id, rev_num, actor, summary, forward_ops, inverse_ops, affected_block_ids)
               VALUES ($1, $2, $3, 'upsert_sources', $4, $5, '{}')"#,
            doc_id.as_str(),
            new_revision.get(),
            actor,
            forward_json,
            inverse_json
        )
        .execute(&mut *tx)
        .await?;
        tx.commit().await?;

        let event = RevisionEvent::new(
            doc_id.clone(),
            new_revision,
            actor,
            "upsert_sources",
            forward_json,
        );
        if let Err(error) = self.bus.publish_revision(&event).await {
            warn!(%error, doc_id = doc_id.as_str(), revision = %new_revision, "failed to publish source revision");
        }
        self.maybe_snapshot(doc_id.as_str(), new_revision, after.clone());

        let warnings = inline_warnings(&after);
        Ok(UpsertSourcesResult {
            canonical_id_map,
            revision: new_revision,
            changed: true,
            sources: after.sources,
            inline_warnings: warnings,
        })
    }

    /// Convenience for validating a doc before creating it.
    pub fn validate_document_for_create(document: &ReportDocument) -> Result<(), StoreError> {
        validate_doc_id(document.doc_id.as_str())?;

        validate_source_registry(&document.sources)
    }

    /// Create a review thread and its first message without touching revisions or CAS state.
    pub async fn create_comment_thread(
        &self,
        doc_id: &DocId,
        request: NewCommentThread,
    ) -> Result<CommentThread, StoreError> {
        validate_comment_body(&request.body)?;
        if let Some(quote) = &request.quote {
            quote.validate().map_err(StoreError::InvalidRequest)?;
        }
        let mut tx = self.pool.begin().await?;
        let loaded = load_document_in_tx(&mut tx, doc_id.as_str()).await?;
        let document = loaded.document;
        let index = document
            .blocks
            .iter()
            .position(|block| block.id == request.block_id)
            .ok_or_else(|| StoreError::CommentTargetNotFound {
                doc_id: doc_id.clone(),
                block_id: request.block_id.clone(),
            })?;
        let block =
            document
                .blocks
                .get(index)
                .ok_or_else(|| StoreError::CommentTargetNotFound {
                    doc_id: doc_id.clone(),
                    block_id: request.block_id.clone(),
                })?;
        let section_id = comment_section_for_index(&document, index);
        let anchor_snapshot = comment_anchor_snapshot(block);
        let quote = request
            .quote
            .as_ref()
            .map(serde_json::to_value)
            .transpose()?;
        let now = Utc::now();
        let thread_id = generate_comment_id("th");
        let message_id = generate_comment_id("msg");
        let status = CommentStatus::Open;

        sqlx::query!(
            r#"INSERT INTO report_comment_threads
               (doc_id, thread_id, block_id, section_id, anchor_snapshot, quote, status,
                created_rev, addressed_rev, created_at, updated_at)
               VALUES ($1, $2, $3, $4, $5, $6, $7, $8, NULL, $9, $9)"#,
            doc_id.as_str(),
            &thread_id,
            request.block_id.as_str(),
            section_id.as_ref().map(BlockId::as_str),
            anchor_snapshot.as_deref(),
            quote,
            status.as_str(),
            document.revision.get(),
            now,
        )
        .execute(&mut *tx)
        .await?;
        sqlx::query!(
            r#"INSERT INTO report_comment_messages
               (message_id, doc_id, thread_id, author_type, author, body, created_at)
               VALUES ($1, $2, $3, $4, $5, $6, $7)"#,
            &message_id,
            doc_id.as_str(),
            &thread_id,
            request.author_type.as_str(),
            &request.author,
            &request.body,
            now,
        )
        .execute(&mut *tx)
        .await?;
        tx.commit().await?;

        Ok(CommentThread {
            thread_id: thread_id.clone(),
            doc_id: doc_id.clone(),
            block_id: request.block_id,
            section_id,
            anchor_snapshot,
            quote: request.quote,
            status,
            created_rev: document.revision,
            addressed_rev: None,
            anchor_detached: false,
            messages: vec![CommentMessage {
                message_id,
                thread_id,
                author_type: request.author_type,
                author: request.author,
                body: request.body,
                created_at: now,
            }],
            created_at: now,
            updated_at: now,
        })
    }

    /// List review threads with live detached-anchor detection.
    pub async fn list_comment_threads(
        &self,
        doc_id: &DocId,
        status: Option<CommentStatus>,
        block_id: Option<&BlockId>,
    ) -> Result<Vec<CommentThread>, StoreError> {
        let rows = sqlx::query!(
            r#"SELECT t.doc_id, t.thread_id, t.block_id, t.section_id, t.anchor_snapshot,
                      t.quote, t.status, t.created_rev, t.addressed_rev, t.created_at,
                      t.updated_at, (b.block_id IS NULL) AS "anchor_detached!"
               FROM report_comment_threads t
               LEFT JOIN report_blocks b
                 ON b.doc_id = t.doc_id AND b.block_id = t.block_id
               WHERE t.doc_id = $1
                 AND ($2::text IS NULL OR t.status = $2)
                 AND ($3::text IS NULL OR t.block_id = $3)
               ORDER BY t.updated_at DESC, t.thread_id ASC"#,
            doc_id.as_str(),
            status.map(CommentStatus::as_str),
            block_id.map(BlockId::as_str),
        )
        .fetch_all(&self.pool)
        .await?;
        let message_rows = sqlx::query!(
            r#"SELECT message_id, thread_id, author_type, author, body, created_at
               FROM report_comment_messages
               WHERE doc_id = $1
               ORDER BY created_at ASC, message_id ASC"#,
            doc_id.as_str()
        )
        .fetch_all(&self.pool)
        .await?;
        let mut messages_by_thread: HashMap<String, Vec<CommentMessage>> = HashMap::new();
        for row in message_rows {
            let message = build_comment_message(
                row.message_id,
                row.thread_id.clone(),
                &row.author_type,
                row.author,
                row.body,
                row.created_at,
            )?;
            messages_by_thread
                .entry(row.thread_id)
                .or_default()
                .push(message);
        }
        rows.into_iter()
            .map(|row| {
                let row = CommentThreadRow {
                    doc_id: row.doc_id,
                    thread_id: row.thread_id,
                    block_id: row.block_id,
                    section_id: row.section_id,
                    anchor_snapshot: row.anchor_snapshot,
                    quote: row.quote,
                    status: row.status,
                    created_rev: row.created_rev,
                    addressed_rev: row.addressed_rev,
                    created_at: row.created_at,
                    updated_at: row.updated_at,
                    anchor_detached: row.anchor_detached,
                };
                let messages = messages_by_thread
                    .remove(&row.thread_id)
                    .unwrap_or_default();
                row.into_thread(messages)
            })
            .collect()
    }

    /// Append a reply and optionally move an `open` thread to `addressed`.
    pub async fn reply_comment_thread(
        &self,
        doc_id: &DocId,
        thread_id: &str,
        reply: NewCommentReply,
    ) -> Result<CommentThread, StoreError> {
        validate_comment_body(&reply.body)?;
        if reply.mark_addressed && reply.author_type != CommentAuthorType::Agent {
            return Err(StoreError::InvalidRequest(
                "only agents may mark a comment addressed".into(),
            ));
        }
        let mut tx = self.pool.begin().await?;
        let current_rev_row = sqlx::query!(
            "SELECT current_rev FROM report_documents WHERE doc_id = $1",
            doc_id.as_str()
        )
        .fetch_optional(&mut *tx)
        .await?
        .ok_or_else(|| StoreError::NotFound {
            doc_id: doc_id.clone(),
        })?;
        let current_rev = Revision::new(current_rev_row.current_rev)?;
        let mut thread = self
            .load_comment_thread_in_tx(&mut tx, doc_id, thread_id)
            .await?;
        let now = Utc::now();
        let message_id = generate_comment_id("msg");
        sqlx::query!(
            r#"INSERT INTO report_comment_messages
               (message_id, doc_id, thread_id, author_type, author, body, created_at)
               VALUES ($1, $2, $3, $4, $5, $6, $7)"#,
            &message_id,
            doc_id.as_str(),
            thread_id,
            reply.author_type.as_str(),
            &reply.author,
            &reply.body,
            now,
        )
        .execute(&mut *tx)
        .await?;

        let mut status = thread.status;
        let mut addressed_rev = thread.addressed_rev;
        if reply.mark_addressed && status == CommentStatus::Open {
            let revision = reply.addressed_rev.unwrap_or(current_rev);
            if revision > current_rev {
                return Err(StoreError::InvalidRequest(format!(
                    "addressed_rev {} is ahead of current revision {}",
                    revision.get(),
                    current_rev.get()
                )));
            }
            status = CommentStatus::Addressed;
            addressed_rev = Some(revision);
        }
        sqlx::query!(
            r#"UPDATE report_comment_threads
               SET status = $3, addressed_rev = $4, updated_at = $5
               WHERE doc_id = $1 AND thread_id = $2"#,
            doc_id.as_str(),
            thread_id,
            status.as_str(),
            addressed_rev.map(Revision::get),
            now,
        )
        .execute(&mut *tx)
        .await?;
        tx.commit().await?;

        thread.status = status;
        thread.addressed_rev = addressed_rev;
        thread.updated_at = now;
        thread.messages.push(CommentMessage {
            message_id,
            thread_id: thread_id.to_string(),
            author_type: reply.author_type,
            author: reply.author,
            body: reply.body,
            created_at: now,
        });
        Ok(thread)
    }

    /// Human-only status transition. Only `archived` and `open` are valid here.
    pub async fn update_comment_status(
        &self,
        doc_id: &DocId,
        thread_id: &str,
        status: CommentStatus,
    ) -> Result<CommentThread, StoreError> {
        if !matches!(status, CommentStatus::Open | CommentStatus::Archived) {
            return Err(StoreError::InvalidRequest(
                "REST may only archive a thread or reopen it".into(),
            ));
        }
        let mut tx = self.pool.begin().await?;
        let mut thread = self
            .load_comment_thread_in_tx(&mut tx, doc_id, thread_id)
            .await?;
        let now = Utc::now();
        let addressed_rev = if status == CommentStatus::Open {
            None
        } else {
            thread.addressed_rev
        };
        sqlx::query!(
            r#"UPDATE report_comment_threads
               SET status = $3, addressed_rev = $4, updated_at = $5
               WHERE doc_id = $1 AND thread_id = $2"#,
            doc_id.as_str(),
            thread_id,
            status.as_str(),
            addressed_rev.map(Revision::get),
            now,
        )
        .execute(&mut *tx)
        .await?;
        tx.commit().await?;
        thread.status = status;
        thread.addressed_rev = addressed_rev;
        thread.updated_at = now;
        Ok(thread)
    }

    async fn load_comment_thread_in_tx(
        &self,
        tx: &mut Transaction<'_, Postgres>,
        doc_id: &DocId,
        thread_id: &str,
    ) -> Result<CommentThread, StoreError> {
        let row = sqlx::query!(
            r#"SELECT t.doc_id, t.thread_id, t.block_id, t.section_id, t.anchor_snapshot,
                      t.quote, t.status, t.created_rev, t.addressed_rev, t.created_at,
                      t.updated_at, (b.block_id IS NULL) AS "anchor_detached!"
               FROM report_comment_threads t
               LEFT JOIN report_blocks b
                 ON b.doc_id = t.doc_id AND b.block_id = t.block_id
               WHERE t.doc_id = $1 AND t.thread_id = $2"#,
            doc_id.as_str(),
            thread_id,
        )
        .fetch_optional(&mut **tx)
        .await?
        .ok_or_else(|| StoreError::CommentThreadNotFound {
            doc_id: doc_id.clone(),
            thread_id: thread_id.to_string(),
        })?;
        let message_rows = sqlx::query!(
            r#"SELECT message_id, thread_id, author_type, author, body, created_at
               FROM report_comment_messages
               WHERE doc_id = $1 AND thread_id = $2
               ORDER BY created_at ASC, message_id ASC"#,
            doc_id.as_str(),
            thread_id,
        )
        .fetch_all(&mut **tx)
        .await?;
        let messages = message_rows
            .into_iter()
            .map(|row| {
                build_comment_message(
                    row.message_id,
                    row.thread_id,
                    &row.author_type,
                    row.author,
                    row.body,
                    row.created_at,
                )
            })
            .collect::<Result<Vec<_>, _>>()?;
        CommentThreadRow {
            doc_id: row.doc_id,
            thread_id: row.thread_id,
            block_id: row.block_id,
            section_id: row.section_id,
            anchor_snapshot: row.anchor_snapshot,
            quote: row.quote,
            status: row.status,
            created_rev: row.created_rev,
            addressed_rev: row.addressed_rev,
            created_at: row.created_at,
            updated_at: row.updated_at,
            anchor_detached: row.anchor_detached,
        }
        .into_thread(messages)
    }

    /// Rebuild a read-only document state at any committed revision.
    ///
    /// Prefers the newest snapshot at or below the target and replays forward ops. When no
    /// snapshot is available, rewinds the current state with inverse ops.
    pub async fn document_at_revision(
        &self,
        doc_id: &DocId,
        revision: Revision,
    ) -> Result<ReportDocument, StoreError> {
        let current = self.get_document(doc_id).await?;
        if revision > current.revision {
            return Err(StoreError::InvalidRequest(format!(
                "revision {} is ahead of current revision {} for `{doc_id}`",
                revision.get(),
                current.revision.get()
            )));
        }
        if revision.is_zero() {
            let mut empty = ReportDocument::new(doc_id.clone());
            empty.revision = Revision::ZERO;
            return Ok(empty);
        }
        if revision == current.revision {
            return Ok(current);
        }
        let snapshot = sqlx::query!(
            r#"SELECT rev_num, snapshot_zstd FROM report_snapshots
               WHERE doc_id = $1 AND rev_num <= $2
               ORDER BY rev_num DESC LIMIT 1"#,
            doc_id.as_str(),
            revision.get()
        )
        .fetch_optional(&self.pool)
        .await?;
        if let Some(snapshot) = snapshot {
            let decoded = zstd::decode_all(snapshot.snapshot_zstd.as_slice())
                .map_err(|error| StoreError::Snapshot(error.to_string()))?;
            let mut document: ReportDocument = serde_json::from_slice(&decoded)?;
            document.revision = Revision::new(snapshot.rev_num)?;
            let rows = sqlx::query!(
                r#"SELECT rev_num, forward_ops FROM report_revisions
                   WHERE doc_id = $1 AND rev_num > $2 AND rev_num <= $3
                   ORDER BY rev_num ASC"#,
                doc_id.as_str(),
                snapshot.rev_num,
                revision.get()
            )
            .fetch_all(&self.pool)
            .await?;
            for row in rows {
                let ops: Vec<PatchOp> = serde_json::from_value(row.forward_ops)?;
                apply_operations_in_place(&mut document, &ops)?;
                document.revision = Revision::new(row.rev_num)?;
            }
            return Ok(document);
        }

        let rows = sqlx::query!(
            r#"SELECT rev_num, inverse_ops FROM report_revisions
               WHERE doc_id = $1 AND rev_num > $2 AND rev_num <= $3
               ORDER BY rev_num DESC"#,
            doc_id.as_str(),
            revision.get(),
            current.revision.get()
        )
        .fetch_all(&self.pool)
        .await?;
        let mut document = current;
        for row in rows {
            let ops: Vec<PatchOp> = serde_json::from_value(row.inverse_ops)?;
            apply_operations_in_place(&mut document, &ops)?;
            document.revision = Revision::new(row.rev_num.saturating_sub(1))?;
        }
        Ok(document)
    }

    fn maybe_snapshot(&self, doc_id: &str, revision: Revision, document: ReportDocument) {
        if revision.get() % 50 != 0 {
            return;
        }
        let pool = self.pool.clone();
        let doc_id = doc_id.to_string();
        tokio::spawn(async move {
            let result = async {
                let json = serde_json::to_vec(&document)?;
                let compressed = zstd::encode_all(json.as_slice(), 5)
                    .map_err(|error| StoreError::Snapshot(error.to_string()))?;
                sqlx::query!(
                    r#"INSERT INTO report_snapshots (doc_id, rev_num, snapshot_zstd)
                       VALUES ($1, $2, $3)
                       ON CONFLICT (doc_id, rev_num) DO NOTHING"#,
                    &doc_id,
                    revision.get(),
                    compressed
                )
                .execute(&pool)
                .await?;
                Ok::<(), StoreError>(())
            }
            .await;
            if let Err(error) = result {
                error!(%error, %doc_id, revision = revision.get(), "failed to persist snapshot");
            }
        });
    }
}

impl CommentThreadRow {
    fn into_thread(self, messages: Vec<CommentMessage>) -> Result<CommentThread, StoreError> {
        Ok(CommentThread {
            thread_id: self.thread_id,
            doc_id: DocId::parse(self.doc_id)?,
            block_id: BlockId::parse(self.block_id)?,
            section_id: self.section_id.map(BlockId::parse).transpose()?,
            anchor_snapshot: self.anchor_snapshot,
            quote: self
                .quote
                .map(|value| parse_json(value, "report_comment_threads.quote"))
                .transpose()?,
            status: CommentStatus::parse(&self.status).ok_or_else(|| {
                StoreError::Internal(format!(
                    "invalid comment status `{}` persisted in database",
                    self.status
                ))
            })?,
            created_rev: Revision::new(self.created_rev)?,
            addressed_rev: self.addressed_rev.map(Revision::new).transpose()?,
            anchor_detached: self.anchor_detached,
            messages,
            created_at: self.created_at,
            updated_at: self.updated_at,
        })
    }
}

fn build_comment_message(
    message_id: String,
    thread_id: String,
    author_type: &str,
    author: String,
    body: String,
    created_at: DateTime<Utc>,
) -> Result<CommentMessage, StoreError> {
    let author_type = CommentAuthorType::parse(author_type).ok_or_else(|| {
        StoreError::Internal(format!(
            "invalid comment author type `{author_type}` persisted in database"
        ))
    })?;
    Ok(CommentMessage {
        message_id,
        thread_id,
        author_type,
        author,
        body,
        created_at,
    })
}

fn comment_section_for_index(document: &ReportDocument, index: usize) -> Option<BlockId> {
    let block = document.blocks.get(index)?;
    if matches!(block.body, BlockKind::Heading { .. }) {
        return Some(block.id.clone());
    }
    document
        .blocks
        .iter()
        .take(index)
        .rev()
        .find(|candidate| matches!(candidate.body, BlockKind::Heading { .. }))
        .map(|heading| heading.id.clone())
}

fn comment_anchor_snapshot(block: &Block) -> Option<String> {
    let text = block.visible_text();
    let text = text.trim();
    if text.is_empty() {
        return None;
    }
    let mut chars = text.chars();
    let mut snapshot: String = chars.by_ref().take(160).collect();
    if chars.next().is_some() {
        snapshot.push('…');
    }
    Some(snapshot)
}

fn validate_comment_body(body: &str) -> Result<(), StoreError> {
    if body.trim().is_empty() {
        return Err(StoreError::InvalidRequest(
            "comment body must not be empty".into(),
        ));
    }
    if body.chars().count() > 20_000 {
        return Err(StoreError::InvalidRequest(
            "comment body must be at most 20000 characters".into(),
        ));
    }
    Ok(())
}

fn generate_comment_id(prefix: &str) -> String {
    format!("{prefix}_{}", Uuid::new_v4().simple())
}

async fn lock_current_revision(
    tx: &mut Transaction<'_, Postgres>,
    doc_id: &str,
) -> Result<Revision, StoreError> {
    let typed_doc_id = DocId::parse(doc_id)?;
    let row = sqlx::query!(
        "SELECT current_rev FROM report_documents WHERE doc_id = $1 FOR UPDATE",
        doc_id
    )
    .fetch_optional(&mut **tx)
    .await?
    .ok_or_else(|| StoreError::NotFound {
        doc_id: typed_doc_id,
    })?;
    Ok(Revision::new(row.current_rev)?)
}

async fn load_document_in_tx(
    tx: &mut Transaction<'_, Postgres>,
    doc_id: &str,
) -> Result<LoadedDocument, StoreError> {
    let typed_doc_id = DocId::parse(doc_id)?;
    let row = sqlx::query!(
        r#"SELECT doc_id, session_id, current_rev, cover, toc, theme,
                  "references" AS "references_json!"
           FROM report_documents WHERE doc_id = $1"#,
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
    };

    let source_rows = sqlx::query!(
        "SELECT source_id, metadata, excerpt FROM report_sources WHERE doc_id = $1 ORDER BY source_id",
        doc_id
    )
    .fetch_all(&mut **tx)
    .await?;
    let mut sources = IndexMap::new();
    for row in source_rows {
        let mut entry: SourceEntry = parse_json(row.metadata, "report_sources.metadata")?;
        if entry.excerpt.is_none() {
            entry.excerpt = row.excerpt;
        }
        sources.insert(SourceId::parse(row.source_id)?, entry);
    }

    let block_rows = sqlx::query!(
        r#"SELECT block_id, rank, payload FROM report_blocks
           WHERE doc_id = $1 ORDER BY rank COLLATE "C""#,
        doc_id
    )
    .fetch_all(&mut **tx)
    .await?;
    let mut blocks = Vec::with_capacity(block_rows.len());
    let mut ranks = BTreeMap::new();
    for row in block_rows {
        let block: Block = parse_json(row.payload, "report_blocks.payload")?;
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
        revision: doc_row.current_rev,
        cover: parse_json(doc_row.cover, "report_documents.cover")?,
        toc: parse_json(doc_row.toc, "report_documents.toc")?,
        theme: parse_json(doc_row.theme, "report_documents.theme")?,
        sources,
        blocks,
        references: parse_json(doc_row.references, "report_documents.references")?,
    };
    Ok(LoadedDocument { document, ranks })
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

fn affected_block_ids(outcome: &PatchOutcome, diff: &DocumentDiff) -> Vec<String> {
    let mut ids: BTreeSet<BlockId> = outcome.affected_block_ids.iter().cloned().collect();
    ids.extend(diff.deleted_block_ids.iter().cloned());
    ids.extend(diff.changed_blocks.iter().map(|block| block.id.clone()));
    let ids: Vec<BlockId> = ids.into_iter().collect();
    block_ids_to_strings(&ids)
}

fn block_ids_to_strings(ids: &[BlockId]) -> Vec<String> {
    ids.iter().map(ToString::to_string).collect()
}

fn source_ids_to_strings(ids: &[SourceId]) -> Vec<String> {
    ids.iter().map(ToString::to_string).collect()
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

fn assign_new_ranks(
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

async fn insert_block_row(
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

async fn persist_blocks(
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

async fn persist_sources(
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

async fn update_document_row(
    tx: &mut Transaction<'_, Postgres>,
    doc_id: &str,
    document: &ReportDocument,
    revision: Revision,
) -> Result<(), StoreError> {
    sqlx::query!(
        r#"UPDATE report_documents
           SET current_rev = $2, cover = $3, toc = $4, theme = $5,
               "references" = $6, updated_at = NOW()
           WHERE doc_id = $1"#,
        doc_id,
        revision.get(),
        serde_json::to_value(&document.cover)?,
        serde_json::to_value(&document.toc)?,
        serde_json::to_value(&document.theme)?,
        serde_json::to_value(&document.references)?
    )
    .execute(&mut **tx)
    .await?;
    Ok(())
}

/// Generate operations that transform `after` back into `before` (including metadata).
pub fn generate_inverse_ops(
    before: &ReportDocument,
    after: &ReportDocument,
) -> Result<Vec<PatchOp>, StoreError> {
    let mut ops = Vec::new();
    if before.cover != after.cover {
        ops.push(PatchOp::SetCover {
            cover: before.cover.clone(),
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
    if before.theme != after.theme {
        ops.push(PatchOp::SetTheme {
            theme: before.theme.clone(),
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

fn inline_warnings(document: &ReportDocument) -> InlineWarnings {
    lint_document(document).diagnostics
}

fn parse_json<T: DeserializeOwned>(value: Value, path: &str) -> Result<T, StoreError> {
    serde_json::from_value(value)
        .map_err(|error| StoreError::Internal(format!("invalid JSON at {path}: {error}")))
}

fn is_unique_violation(error: &sqlx::Error) -> bool {
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

type SourceMergeResult = (
    IndexMap<SourceId, SourceEntry>,
    BTreeMap<SourceId, SourceId>,
);

fn merge_source_upserts(
    existing: &IndexMap<SourceId, SourceEntry>,
    upserts: &[SourceUpsert],
) -> Result<SourceMergeResult, StoreError> {
    let mut sources = existing.clone();
    let mut key_to_id: BTreeMap<String, SourceId> = sources
        .iter()
        .map(|(source_id, source)| (normalized_source_key(source), source_id.clone()))
        .collect();
    let mut canonical_map = BTreeMap::new();
    for upsert in upserts {
        let normalized = normalized_source_key(&upsert.source);
        let canonical_id = if let Some(existing_id) = key_to_id.get(&normalized) {
            existing_id.clone()
        } else {
            upsert.source_id.clone()
        };
        canonical_map.insert(upsert.source_id.clone(), canonical_id.clone());
        if canonical_id != upsert.source_id {
            if let Some(existing_source) = sources.get(&canonical_id).cloned() {
                let merged = merge_source(&existing_source, &upsert.source);
                key_to_id.insert(normalized_source_key(&merged), canonical_id.clone());
                sources.insert(canonical_id.clone(), merged);
            }
            continue;
        }
        if let Some(existing_source) = sources.get(&upsert.source_id) {
            let old_key = normalized_source_key(existing_source);
            key_to_id.remove(&old_key);
        }
        let merged = sources
            .get(&upsert.source_id)
            .map(|existing_source| merge_source(existing_source, &upsert.source))
            .unwrap_or_else(|| upsert.source.clone());
        key_to_id.insert(normalized_source_key(&merged), canonical_id.clone());
        sources.insert(canonical_id, merged);
    }
    validate_source_registry(&sources)?;
    Ok((sources, canonical_map))
}

pub fn normalize_title(title: &str) -> String {
    let mut output = String::new();
    let mut pending_space = false;
    for ch in title.chars().flat_map(char::to_lowercase) {
        if ch.is_alphanumeric() {
            if pending_space && !output.is_empty() {
                output.push(' ');
            }
            pending_space = false;
            output.push(ch);
        } else {
            pending_space = true;
        }
    }
    if output.is_empty() {
        "untitled".to_string()
    } else {
        output
    }
}

pub fn normalized_source_key(source: &SourceEntry) -> String {
    if let Some(url) = source
        .url
        .as_deref()
        .map(str::trim)
        .filter(|url| !url.is_empty())
    {
        let normalized = url.trim_end_matches('/').to_ascii_lowercase();
        return format!("url:{normalized}");
    }
    format!("title:{}", normalize_title(&source.title))
}

/// Merge two source entries, preferring non-empty fields from the incoming entry.
pub fn merge_source_entries(existing: &SourceEntry, incoming: &SourceEntry) -> SourceEntry {
    merge_source(existing, incoming)
}

fn merge_source(existing: &SourceEntry, incoming: &SourceEntry) -> SourceEntry {
    SourceEntry {
        title: if incoming.title.trim().is_empty() {
            existing.title.clone()
        } else {
            incoming.title.clone()
        },
        url: incoming
            .url
            .clone()
            .filter(|value| !value.trim().is_empty())
            .or_else(|| existing.url.clone()),
        authors: if incoming.authors.is_empty() {
            existing.authors.clone()
        } else {
            incoming.authors.clone()
        },
        publisher: incoming
            .publisher
            .clone()
            .filter(|value| !value.trim().is_empty())
            .or_else(|| existing.publisher.clone()),
        date: incoming
            .date
            .clone()
            .filter(|value| !value.trim().is_empty())
            .or_else(|| existing.date.clone()),
        accessed: incoming
            .accessed
            .clone()
            .filter(|value| !value.trim().is_empty())
            .or_else(|| existing.accessed.clone()),
        note: incoming
            .note
            .clone()
            .filter(|value| !value.trim().is_empty())
            .or_else(|| existing.note.clone()),
        excerpt: incoming
            .excerpt
            .clone()
            .filter(|value| !value.trim().is_empty())
            .or_else(|| existing.excerpt.clone()),
    }
}

fn validate_source_registry(sources: &IndexMap<SourceId, SourceEntry>) -> Result<(), StoreError> {
    let mut seen = BTreeMap::new();
    for (source_id, source) in sources {
        let key = normalized_source_key(source);
        if let Some(existing) = seen.insert(key.clone(), source_id.clone()) {
            return Err(StoreError::SourceConflict {
                source_id: source_id.clone(),
                normalized_key: key,
                canonical_id: existing,
            });
        }
    }
    Ok(())
}

/// Compute the block IDs affected by a list of operations (used by tests and Smart CAS diagnostics).
pub fn touched_block_ids(ops: &[PatchOp]) -> BTreeSet<BlockId> {
    ops.iter().flat_map(PatchOp::touched_block_ids).collect()
}

/// Keep exports used by the store integration tests.
pub use quire_protocol::rank::{rank_between, ranks_for_count};

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
