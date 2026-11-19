//! Shared server state and request metadata helpers.

use std::sync::Arc;

use chrono::Utc;
use quire_protocol::{CommentStatus, CommentThread, CommentUpdatedEvent, DocId, EphemeralPayload};
use quire_store::PgReportStore;

use crate::config::Config;

#[derive(Clone)]
pub struct AppState {
    pub store: Arc<PgReportStore>,
    pub config: Arc<Config>,
}

impl AppState {
    pub fn new(store: PgReportStore, config: Config) -> Self {
        Self {
            store: Arc::new(store),
            config: Arc::new(config),
        }
    }

    /// Best-effort live notification for a review-thread mutation.
    pub async fn publish_comment_updated(&self, thread: &CommentThread) {
        let event = CommentUpdatedEvent {
            doc_id: thread.doc_id.clone(),
            thread_id: thread.thread_id.clone(),
            status: thread.status,
            anchor_detached: thread.anchor_detached,
            updated_at: thread.updated_at,
        };
        let payload = EphemeralPayload::CommentUpdated { event };
        if let Err(error) = self
            .store
            .bus()
            .publish_ephemeral(thread.doc_id.as_str(), &payload)
            .await
        {
            tracing::warn!(
                %error,
                doc_id = thread.doc_id.as_str(),
                thread_id = thread.thread_id.as_str(),
                "failed to publish comment_updated"
            );
        }
    }

    /// Best-effort live notification after a thread is deleted.
    pub async fn publish_comment_deleted(&self, doc_id: &DocId, thread_id: &str) {
        let event = CommentUpdatedEvent {
            doc_id: doc_id.clone(),
            thread_id: thread_id.to_string(),
            status: CommentStatus::Archived,
            anchor_detached: true,
            updated_at: Utc::now(),
        };
        let payload = EphemeralPayload::CommentUpdated { event };
        if let Err(error) = self
            .store
            .bus()
            .publish_ephemeral(doc_id.as_str(), &payload)
            .await
        {
            tracing::warn!(
                %error,
                doc_id = doc_id.as_str(),
                thread_id,
                "failed to publish comment_deleted"
            );
        }
    }
}
