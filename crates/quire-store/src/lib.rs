//! PostgreSQL storage, Redis Streams event bus and revision engine.

#![forbid(unsafe_code)]

pub mod error;
pub mod pg_store;
pub mod redis_stream;
pub mod types;

pub use error::{RevisionConflict, StoreError};
pub use pg_store::{
    PgReportStore, generate_inverse_ops, merge_source_entries, normalize_title,
    normalized_source_key, validate_doc_id, validate_session_id, validate_source_id,
};
pub use redis_stream::{EphemeralSubscriber, RedisStreamBus, StreamReader};
pub use types::{
    ApplyResult, CommitExtras, CreateResult, DeleteSourcesResult, DocumentSnapshot,
    NewCommentReply, NewCommentThread, ReportList, ReportSummary, RevisionEvent, RevisionRecord,
    RollbackResult, SessionEvent, SessionStreamEntry, SourceUpsert, StreamRevision,
    UpsertSourcesResult,
};
