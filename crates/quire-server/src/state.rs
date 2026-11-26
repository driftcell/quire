//! Shared server state, external harness authentication, and session binding hooks.

use std::{future::Future, pin::Pin, sync::Arc};

use chrono::Utc;
use quire_protocol::{
    CommentStatus, CommentThread, CommentUpdatedEvent, CoverConfig, DocId, EphemeralPayload,
    ReferencesConfig, SessionId, ThemeConfig, TocConfig,
};
use quire_store::{PgReportStore, ReportList, StoreError};

use crate::{auth::RequestContext, config::Config};

pub type BoxFuture<'a, T> = Pin<Box<dyn Future<Output = T> + Send + 'a>>;
pub type AccessibleReportScope = Option<(Vec<String>, Vec<String>)>;

/// Verified identity resolved from a user API key or JWT on `/mcp` or document routes.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct VerifiedExternalIdentity {
    pub user_id: String,
    pub username: String,
    pub is_admin: bool,
    pub preview_doc_id: Option<String>,
}

/// Complete report configuration supplied by the runtime for a selected or default style.
#[derive(Debug, Clone, Default)]
pub struct ExternalReportTemplate {
    pub theme: ThemeConfig,
    pub cover: CoverConfig,
    pub toc: TocConfig,
    pub references: ReferencesConfig,
}

/// Transport-neutral image returned by runtime preview tools.
#[derive(Debug, Clone)]
pub struct ExternalPreviewImage {
    pub base64: String,
    pub mime_type: String,
    pub caption: String,
}

/// JSON diagnostics plus an optional screenshot shared by Rig and rmcp adapters.
#[derive(Debug, Clone)]
pub struct ExternalPreviewToolResult {
    pub payload: serde_json::Value,
    pub image: Option<ExternalPreviewImage>,
}

impl ExternalPreviewToolResult {
    /// Convert the shared runtime result to an rmcp JSON diagnostic plus optional PNG content.
    pub fn into_mcp_result(self) -> Result<rmcp::model::CallToolResult, rmcp::ErrorData> {
        let diagnostics = serde_json::to_string_pretty(&self.payload).map_err(|error| {
            rmcp::ErrorData::internal_error(
                format!("failed to serialize preview diagnostics: {error}"),
                None,
            )
        })?;
        let mut content = vec![rmcp::model::ContentBlock::text(diagnostics)];
        if let Some(image) = self.image {
            content.push(rmcp::model::ContentBlock::text(image.caption));
            content.push(rmcp::model::ContentBlock::image(
                image.base64,
                image.mime_type,
            ));
        }
        Ok(rmcp::model::CallToolResult::success(content))
    }
}

/// Pluggable bridge from `quire-server` to `quire-runtime` for API key authentication,
/// live JWT account status verification, document/session ACLs, and automatic empty-session
/// provisioning when an external MCP harness creates a report.
pub trait ExternalHarnessBridge: Send + Sync {
    /// Verify a bearer API key (e.g. `qk_...`) and return its owning user identity if active.
    fn verify_api_key<'a>(
        &'a self,
        raw_token: &'a str,
    ) -> BoxFuture<'a, Option<VerifiedExternalIdentity>>;

    /// Verify that a JWT `sub` still corresponds to an active (`enabled = TRUE`) user in
    /// `identity.users`, returning their current database role.
    fn verify_active_user<'a>(
        &'a self,
        user_id: &'a str,
    ) -> BoxFuture<'a, Option<VerifiedExternalIdentity>>;

    /// Resolve an explicit style preset or the user's default into complete report configuration.
    fn resolve_report_template<'a>(
        &'a self,
        theme_preset: Option<&'a str>,
        user_id: Option<&'a str>,
        is_admin: bool,
    ) -> BoxFuture<'a, Result<Option<ExternalReportTemplate>, String>>;

    fn preview_report<'a>(
        &'a self,
        report: crate::report::QuireReportService,
        context: RequestContext,
        request: crate::report::PreviewReportInput,
    ) -> BoxFuture<'a, Result<ExternalPreviewToolResult, rmcp::ErrorData>>;

    fn preview_echarts<'a>(
        &'a self,
        report: crate::report::QuireReportService,
        context: RequestContext,
        request: crate::report::PreviewEchartsParams,
    ) -> BoxFuture<'a, Result<ExternalPreviewToolResult, rmcp::ErrorData>>;

    fn commit_chart<'a>(
        &'a self,
        report: crate::report::QuireReportService,
        context: RequestContext,
        request: crate::report::CommitChartParams,
    ) -> BoxFuture<'a, Result<ExternalPreviewToolResult, rmcp::ErrorData>>;

    /// Ensure an active session exists in `runtime.sessions` before `create_report` commits,
    /// returning the canonical `SessionId` to store on `report.documents`.
    fn prepare_session_for_report<'a>(
        &'a self,
        requested_session_id: Option<&'a SessionId>,
        user_id: Option<&'a str>,
        is_admin: bool,
        title: &'a str,
    ) -> BoxFuture<'a, Result<SessionId, String>>;

    /// Bind the newly created `doc_id` to `session_id` in `runtime.sessions`.
    fn bind_created_report<'a>(
        &'a self,
        session_id: &'a SessionId,
        user_id: Option<&'a str>,
        is_admin: bool,
        title: &'a str,
        doc_id: &'a DocId,
    ) -> BoxFuture<'a, Result<(), String>>;

    /// Verify whether the caller described by `(user_id, caller_session_id, is_admin)` is allowed
    /// to access `doc_id` (whose bound `doc_session_id` was read from `report.documents`).
    fn authorize_document_access<'a>(
        &'a self,
        doc_id: &'a DocId,
        doc_session_id: Option<&'a SessionId>,
        user_id: Option<&'a str>,
        caller_session_id: Option<&'a SessionId>,
        is_admin: bool,
    ) -> BoxFuture<'a, Result<(), String>>;

    /// Verify whether the caller is allowed to subscribe to or access `session_id`.
    fn authorize_session_access<'a>(
        &'a self,
        session_id: &'a SessionId,
        user_id: Option<&'a str>,
        is_admin: bool,
    ) -> BoxFuture<'a, Result<(), String>>;

    /// Return `(owned_session_ids, owned_doc_ids)` when listing reports for a non-admin caller
    /// (`None` means unrestricted, e.g. for administrators or system tokens).
    fn accessible_report_scope<'a>(
        &'a self,
        user_id: Option<&'a str>,
        caller_session_id: Option<&'a SessionId>,
        is_admin: bool,
    ) -> BoxFuture<'a, Result<AccessibleReportScope, String>>;
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

    /// Enforce document-level ACL for both REST `/api/docs/*`, `/mcp`, and in-process tools.
    pub async fn authorize_doc_access(
        &self,
        context: &RequestContext,
        doc_id: &DocId,
        is_write: bool,
    ) -> Result<(), StoreError> {
        if let Some(preview_doc_id) = context.preview_doc_id.as_deref() {
            if is_write || preview_doc_id != doc_id.as_str() {
                return Err(StoreError::Forbidden(format!(
                    "preview token is only authorized for read-only access to `{preview_doc_id}`"
                )));
            }
            return Ok(());
        }
        let doc_session_id = self.store.get_document_session_id(doc_id).await?;
        if let Some(bridge) = self.harness_bridge.as_ref() {
            if context.user_id.is_some() || context.session_id.is_some() {
                bridge
                    .authorize_document_access(
                        doc_id,
                        doc_session_id.as_ref(),
                        context.user_id.as_deref(),
                        context.session_id.as_ref(),
                        context.is_admin,
                    )
                    .await
                    .map_err(StoreError::Forbidden)?;
            }
            return Ok(());
        }
        if !context.is_admin
            && let (Some(caller_sid), Some(doc_sid)) = (&context.session_id, &doc_session_id)
            && caller_sid != doc_sid
        {
            return Err(StoreError::Forbidden(format!(
                "session `{caller_sid}` does not own document `{doc_id}`"
            )));
        }
        Ok(())
    }

    /// Enforce session-level ACL on legacy session SSE routes.
    pub async fn authorize_session_access(
        &self,
        context: &RequestContext,
        session_id: &SessionId,
    ) -> Result<(), StoreError> {
        if context.preview_doc_id.is_some() {
            return Err(StoreError::Forbidden(
                "preview token cannot access session streams".into(),
            ));
        }
        if let Some(bridge) = self.harness_bridge.as_ref()
            && context.user_id.is_some()
        {
            bridge
                .authorize_session_access(session_id, context.user_id.as_deref(), context.is_admin)
                .await
                .map_err(StoreError::Forbidden)?;
        }
        Ok(())
    }

    /// List reports filtered by the caller's tenant/session scope.
    pub async fn list_reports_authorized(
        &self,
        context: &RequestContext,
        query: Option<&str>,
        limit: i64,
        offset: i64,
    ) -> Result<ReportList, StoreError> {
        if context.preview_doc_id.is_some() {
            return Err(StoreError::Forbidden(
                "preview token cannot list reports".into(),
            ));
        }
        if let Some(bridge) = self.harness_bridge.as_ref()
            && (context.user_id.is_some() || context.session_id.is_some())
            && let Some((sessions, docs)) = bridge
                .accessible_report_scope(
                    context.user_id.as_deref(),
                    context.session_id.as_ref(),
                    context.is_admin,
                )
                .await
                .map_err(StoreError::Forbidden)?
        {
            return self
                .store
                .list_reports_for_scope(query, limit, offset, &sessions, &docs)
                .await;
        }
        self.store.list_reports(query, limit, offset).await
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
