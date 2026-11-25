//! Shared server state, external harness authentication, and session binding hooks.

use std::{future::Future, pin::Pin, sync::Arc};

use chrono::Utc;
use quire_protocol::{
    CommentStatus, CommentThread, CommentUpdatedEvent, DocId, EphemeralPayload, SessionId,
};
use quire_store::PgReportStore;

use crate::config::Config;

pub type BoxFuture<'a, T> = Pin<Box<dyn Future<Output = T> + Send + 'a>>;

/// Verified identity resolved from a user API key or JWT on `/mcp` or document routes.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct VerifiedExternalIdentity {
    pub user_id: String,
    pub username: String,
}

/// Pluggable bridge from `quire-server` to `quire-runtime` for API key authentication and
/// automatic empty-session provisioning when an external MCP harness creates a report.
pub trait ExternalHarnessBridge: Send + Sync {
    /// Verify a bearer API key (e.g. `qk_...`) and return its owning user identity if active.
    fn verify_api_key<'a>(
        &'a self,
        raw_token: &'a str,
    ) -> BoxFuture<'a, Option<VerifiedExternalIdentity>>;

    /// Ensure an active session exists in `runtime.sessions` before `create_report` commits,
    /// returning the canonical `SessionId` to store on `report.documents`.
    fn prepare_session_for_report<'a>(
        &'a self,
        requested_session_id: Option<&'a SessionId>,
        user_id: Option<&'a str>,
        title: &'a str,
    ) -> BoxFuture<'a, Result<SessionId, String>>;

    /// Bind the newly created `doc_id` to `session_id` in `runtime.sessions`.
    fn bind_created_report<'a>(
        &'a self,
        session_id: &'a SessionId,
        user_id: Option<&'a str>,
        title: &'a str,
        doc_id: &'a DocId,
    ) -> BoxFuture<'a, Result<(), String>>;
}

#[derive(Clone)]
pub struct AppState {
    pub store: Arc<PgReportStore>,
    pub config: Arc<Config>,
    pub harness_bridge: Option<Arc<dyn ExternalHarnessBridge>>,
}

impl AppState {
    pub fn new(store: PgReportStore, config: Config) -> Self {
        Self {
            store: Arc::new(store),
            config: Arc::new(config),
            harness_bridge: None,
        }
    }

    pub fn with_harness_bridge(mut self, bridge: Arc<dyn ExternalHarnessBridge>) -> Self {
        self.harness_bridge = Some(bridge);
        self
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
