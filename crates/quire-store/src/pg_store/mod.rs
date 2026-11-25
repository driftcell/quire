//! PostgreSQL row-per-block persistence, Smart CAS, revisions and snapshots.

use std::{
    collections::{BTreeMap, BTreeSet, HashMap, HashSet},
    sync::{Arc, Mutex as StdMutex},
};

use chrono::{DateTime, Utc};
use indexmap::IndexMap;
use quire_lint::{lint_document, normalize::normalize_document};
use quire_protocol::{
    Block, BlockId, BlockKind, CommentAuthorType, CommentMessage, CommentStatus, CommentThread,
    DocId, PatchOp, PatchOutcome, Rank, ReportDocument, Revision, SessionId, SourceEntry, SourceId,
    apply_operations_in_place, rank_after, remap_operations_sources, short_id, try_rank_between,
};
use serde::de::DeserializeOwned;
use serde_json::Value;
use sqlx::{PgPool, Postgres, Transaction};
use tokio::sync::{Mutex as AsyncMutex, OwnedMutexGuard};
use tracing::{error, warn};

use crate::{
    error::{InlineWarnings, RevisionConflict, StoreError, ops_to_json},
    redis_stream::RedisStreamBus,
    types::{
        ApplyResult, CommitExtras, CreateResult, DeleteSourcesResult, DocumentDiff,
        DocumentSnapshot, NewCommentReply, NewCommentThread, ReportList, ReportSummary,
        RevisionEvent, RevisionRecord, RollbackResult, SessionEvent, SourceUpsert,
        UpsertSourcesResult,
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
    lint: Value,
}

#[derive(Debug, Clone)]
struct CommentThreadRow {
    doc_id: String,
    thread_id: String,
    block_id: String,
    end_block_id: Option<String>,
    anchor_blocks: Value,
    section_id: Option<String>,
    anchor_snapshot: Option<String>,
    quote: Option<Value>,
    end_quote: Option<Value>,
    status: String,
    created_rev: i64,
    addressed_rev: Option<i64>,
    created_at: DateTime<Utc>,
    updated_at: DateTime<Utc>,
    anchor_detached: bool,
    evaluation_meta: Option<Value>,
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

    pub(crate) async fn document_guard(&self, doc_id: &str) -> OwnedMutexGuard<()> {
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
    ///
    /// Report migrations own the `report` schema and keep their ledger at
    /// `report._sqlx_migrations`, separate from other domain pipelines.
    pub async fn migrate(&self) -> Result<(), StoreError> {
        let mut migrator = sqlx::migrate!("./migrations");
        migrator.create_schema("report");
        migrator.dangerous_set_table_name("report._sqlx_migrations");
        migrator
            .run(&self.pool)
            .await
            .map_err(|error| StoreError::Internal(format!("migration failed: {error}")))
    }
}

mod block_rows;
mod comments;
mod document_crud;
mod smart_cas;
mod snapshots;
mod sources;
mod support;

pub(crate) use block_rows::*;
pub(crate) use document_crud::{load_document_in_tx, lock_current_revision};
pub use smart_cas::generate_inverse_ops;
pub use sources::{merge_source_entries, normalize_title, normalized_source_key};
pub(crate) use sources::{merge_source_upserts, validate_source_registry};
pub(crate) use support::*;
pub use support::{validate_doc_id, validate_session_id, validate_source_id};
