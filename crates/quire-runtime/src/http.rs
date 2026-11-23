//! Axum control plane and Session-SSE endpoint.

use std::{convert::Infallible, time::Duration};

use axum::{
    Json, Router,
    extract::{Path, Query, Request, State},
    http::{StatusCode, header},
    middleware::{self, Next},
    response::{
        IntoResponse, Response,
        sse::{Event, KeepAlive, Sse},
    },
    routing::{get, patch, post},
};
use futures::Stream;
use quire_protocol::{ApiErrorEnvelope, CommentStatus, CommentThread, TextQuoteSelector, short_id};
use quire_session_protocol::{
    AttachmentMeta, CreateSessionRequest, PendingQueueItem, QuestionAnswer, RunTraceSummary,
    SendMessageRequest, SessionCommentRef, SessionDetail, SessionEvent, SessionEventKind,
    SessionSummary, ToolCallDetail, UpdatePendingMessageRequest,
};
use rmcp::transport::streamable_http_server::{
    StreamableHttpServerConfig, StreamableHttpService, session::local::LocalSessionManager,
};
use secrecy::ExposeSecret;
use serde::{Deserialize, Serialize};
use subtle::ConstantTimeEq;
use tokio_util::sync::CancellationToken;
use tower_http::cors::{Any, CorsLayer};
use tracing::warn;

use crate::{
    admin_mcp::AdminMcpServer,
    error::{RuntimeError, RuntimeResult},
    kb::{
        AgentDefinitionRecord, KbBranchDiff, KbBranchRecord, KbDocumentRecord, KbPromoteReport,
        KbRollbackReport, SkillDefinitionRecord, UpsertKbAgentCommand,
    },
    mcp_registry::{McpServerInput, McpServerPatch, McpServerRecord, McpSyncReport, McpToolRecord},
    models::{
        ModelInput, ModelRecord, ModelsDevImportReport, ModelsDevImportRequest, ProviderInput,
        ProviderRecord,
    },
    preview::ChartPreviewRecord,
    service::{AttachmentPresign, AttachmentWrite, PostMessageOutcome, RuntimeService},
    store::{SessionEventFrame, SessionPatch},
};

pub fn router(service: RuntimeService) -> Router {
    let mcp_config = StreamableHttpServerConfig::default()
        .with_allowed_hosts(service.config().mcp_allowed_hosts())
        .with_json_response(true)
        .with_sse_keep_alive(Some(Duration::from_secs(15)))
        .with_cancellation_token(CancellationToken::new());
    let mcp_state = service.clone();
    let admin_mcp: StreamableHttpService<AdminMcpServer, LocalSessionManager> =
        StreamableHttpService::new(
            move || Ok(AdminMcpServer::new(mcp_state.clone())),
            Default::default(),
            mcp_config,
        );
    let admin_mcp = Router::new()
        .nest_service("/admin/mcp", admin_mcp)
        .route_layer(middleware::from_fn_with_state(
            service.clone(),
            runtime_auth,
        ));
    let chart_previews = Router::new()
        .route("/api/chart-previews/{preview_id}", get(get_chart_preview))
        .route_layer(middleware::from_fn_with_state(
            service.clone(),
            chart_preview_auth,
        ));
    let protected = Router::new()
        .route("/api/sessions", get(list_sessions).post(create_session))
        .route(
            "/api/sessions/{session_id}",
            get(get_session)
                .patch(update_session)
                .delete(delete_session),
        )
        .route("/api/sessions/{session_id}/events", get(session_events))
        .route(
            "/api/sessions/{session_id}/prune-events",
            post(prune_events),
        )
        .route(
            "/api/sessions/{session_id}/traces",
            get(list_session_traces),
        )
        .route("/api/sessions/{session_id}/messages", post(post_message))
        .route(
            "/api/sessions/{session_id}/comments",
            post(create_session_comment),
        )
        .route(
            "/api/sessions/{session_id}/comments/{thread_id}",
            axum::routing::delete(delete_session_comment).patch(update_session_comment_status),
        )
        .route(
            "/api/sessions/{session_id}/comments/{thread_id}/replies",
            post(reply_session_comment),
        )
        .route("/api/sessions/{session_id}/queue", get(list_session_queue))
        .route(
            "/api/sessions/{session_id}/queue/{queue_id}",
            patch(update_session_queue_item).delete(delete_session_queue_item),
        )
        .route(
            "/api/sessions/{session_id}/questions/{question_id}/answer",
            post(answer_question),
        )
        .route("/api/sessions/{session_id}/interrupt", post(interrupt))
        .route("/api/sessions/{session_id}/retry", post(retry))
        .route("/api/sessions/{session_id}/compact", post(compact))
        .route(
            "/api/sessions/{session_id}/attachments",
            post(post_session_attachment),
        )
        .route(
            "/api/sessions/{session_id}/attachments/presign",
            post(presign_session_attachment),
        )
        .route("/attachments", post(post_attachment))
        .route(
            "/api/sessions/{session_id}/tool-calls/{tool_call_id}",
            get(get_tool_call),
        )
        .route("/api/kb/docs", get(list_kb_docs).post(create_kb_doc))
        .route(
            "/api/kb/docs/{doc_id}",
            patch(update_kb_doc).delete(delete_kb_doc),
        )
        .route(
            "/api/kb/branches",
            get(list_kb_branches).post(create_kb_branch),
        )
        .route("/api/kb/branches/{name}/diff", get(diff_kb_branch))
        .route("/api/kb/branches/{name}/promote", post(promote_kb_branch))
        .route("/api/kb/rollback", post(rollback_kb))
        .route("/api/kb/agents", get(list_agents).post(create_agent))
        .route(
            "/api/kb/agents/{name}",
            patch(update_agent).delete(delete_agent),
        )
        .route("/api/kb/skills", get(list_skills).post(create_skill))
        .route(
            "/api/kb/skills/{name}",
            patch(update_skill).delete(delete_skill),
        )
        .route(
            "/api/models/providers",
            get(list_model_providers).post(create_model_provider),
        )
        .route(
            "/api/models/providers/{provider_id}",
            patch(update_model_provider).delete(delete_model_provider),
        )
        .route("/api/models", get(list_models).post(create_model))
        .route(
            "/api/models/{*id}",
            patch(update_model).delete(delete_model),
        )
        .route("/api/models/import-models-dev", post(import_models_dev))
        .route(
            "/api/mcp/servers",
            get(list_mcp_servers).post(upsert_mcp_server),
        )
        .route(
            "/api/mcp/servers/{server_id}",
            patch(patch_mcp_server).delete(delete_mcp_server),
        )
        .route("/api/mcp/servers/{server_id}/sync", post(sync_mcp_server))
        .route("/api/mcp/tools", get(list_mcp_tools))
        .route(
            "/api/mcp/servers/{server_id}/tools/{tool_name}",
            patch(set_mcp_tool_enabled),
        )
        .route_layer(middleware::from_fn_with_state(
            service.clone(),
            runtime_auth,
        ));
    let document_router = quire_server::build_document_router(service.report_app_state());
    Router::new()
        .route("/health", get(health))
        .merge(chart_previews)
        .merge(protected)
        .merge(admin_mcp)
        .with_state(service)
        .merge(document_router)
        .layer(
            CorsLayer::new()
                .allow_origin(Any)
                .allow_methods(Any)
                .allow_headers(Any),
        )
}

async fn runtime_auth(
    State(service): State<RuntimeService>,
    request: Request,
    next: Next,
) -> Response {
    if request.method() == axum::http::Method::OPTIONS {
        return next.run(request).await;
    }
    let expected = service.config().api_token.expose_secret();
    if expected.is_empty() {
        return (
            StatusCode::SERVICE_UNAVAILABLE,
            Json(ApiErrorEnvelope::<()>::new(
                "auth_not_configured",
                "runtime API token is not configured",
                "Set RUNTIME_API_TOKEN before exposing quire-runtime.",
                None,
            )),
        )
            .into_response();
    }
    let presented = request
        .headers()
        .get(header::AUTHORIZATION)
        .and_then(|value| value.to_str().ok())
        .and_then(|value| value.strip_prefix("Bearer "))
        .map(str::trim)
        .filter(|value| !value.is_empty());
    let matches = presented.is_some_and(|value| {
        value.len() == expected.len() && bool::from(value.as_bytes().ct_eq(expected.as_bytes()))
    });
    if !matches {
        let mut response = (
            StatusCode::UNAUTHORIZED,
            Json(ApiErrorEnvelope::<()>::new(
                "unauthorized",
                "missing or invalid runtime bearer token",
                "Send `Authorization: Bearer $RUNTIME_API_TOKEN`.",
                None,
            )),
        )
            .into_response();
        response.headers_mut().insert(
            header::WWW_AUTHENTICATE,
            axum::http::HeaderValue::from_static("Bearer"),
        );
        return response;
    }
    next.run(request).await
}

/// Read-only auth for the stateless chart preview route. Accepts the same `ro_token`/`rw_token`
/// pair as `GET /api/docs/{doc_id}` (plus the runtime API token for local development).
async fn chart_preview_auth(
    State(service): State<RuntimeService>,
    request: Request,
    next: Next,
) -> Response {
    if request.method() == axum::http::Method::OPTIONS {
        return next.run(request).await;
    }
    let config = service.config();
    let candidates = [
        &config.quire_ro_token,
        &config.quire_rw_token,
        &config.api_token,
    ];
    if candidates
        .iter()
        .all(|token| token.expose_secret().trim().is_empty())
    {
        return (
            StatusCode::SERVICE_UNAVAILABLE,
            Json(ApiErrorEnvelope::<()>::new(
                "auth_not_configured",
                "chart preview auth is not configured",
                "Set QUIRE_RO_TOKEN, QUIRE_RW_TOKEN or RUNTIME_API_TOKEN.",
                None,
            )),
        )
            .into_response();
    }
    let presented = request
        .headers()
        .get(header::AUTHORIZATION)
        .and_then(|value| value.to_str().ok())
        .and_then(|value| value.strip_prefix("Bearer "))
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .or_else(|| {
            request
                .headers()
                .get("x-quire-token")
                .and_then(|value| value.to_str().ok())
                .map(str::trim)
                .filter(|value| !value.is_empty())
        });
    let matches = presented.is_some_and(|presented| {
        candidates.iter().any(|token| {
            let expected = token.expose_secret().trim();
            !expected.is_empty()
                && expected.len() == presented.len()
                && bool::from(presented.as_bytes().ct_eq(expected.as_bytes()))
        })
    });
    if !matches {
        let mut response = (
            StatusCode::UNAUTHORIZED,
            Json(ApiErrorEnvelope::<()>::new(
                "unauthorized",
                "missing or invalid chart preview token",
                "Send `Authorization: Bearer $QUIRE_RO_TOKEN`.",
                None,
            )),
        )
            .into_response();
        response.headers_mut().insert(
            header::WWW_AUTHENTICATE,
            axum::http::HeaderValue::from_static("Bearer"),
        );
        return response;
    }
    next.run(request).await
}

#[derive(Serialize)]
struct RuntimeHealthOk {
    status: &'static str,
    service: &'static str,
}

#[derive(Serialize)]
struct RuntimeHealthDegraded {
    status: &'static str,
    error: String,
}

async fn health(State(service): State<RuntimeService>) -> Response {
    match service.store().health().await {
        Ok(()) => (
            StatusCode::OK,
            Json(RuntimeHealthOk {
                status: "ok",
                service: "quire-runtime",
            }),
        )
            .into_response(),
        Err(error) => (
            StatusCode::SERVICE_UNAVAILABLE,
            Json(RuntimeHealthDegraded {
                status: "degraded",
                error: error.to_string(),
            }),
        )
            .into_response(),
    }
}

#[derive(Debug, Deserialize)]
struct PageQuery {
    limit: Option<i64>,
    offset: Option<i64>,
}

async fn list_sessions(
    State(service): State<RuntimeService>,
    Query(query): Query<PageQuery>,
) -> RuntimeResult<Json<Vec<SessionSummary>>> {
    service
        .list_sessions(query.limit.unwrap_or(50), query.offset.unwrap_or(0))
        .await
        .map(Json)
}

async fn create_session(
    State(service): State<RuntimeService>,
    Json(request): Json<CreateSessionRequest>,
) -> RuntimeResult<Json<SessionSummary>> {
    service
        .create_session_on_branch(
            &request.title,
            request.doc_id.as_deref(),
            request.kb_branch.as_deref(),
        )
        .await
        .map(Json)
}

async fn get_session(
    State(service): State<RuntimeService>,
    Path(session_id): Path<String>,
) -> RuntimeResult<Json<SessionDetail>> {
    service.session_detail(&session_id).await.map(Json)
}

async fn get_chart_preview(
    State(service): State<RuntimeService>,
    Path(preview_id): Path<String>,
) -> RuntimeResult<Json<ChartPreviewRecord>> {
    service
        .store()
        .bus()
        .load_chart_preview(&preview_id)
        .await?
        .map(Json)
        .ok_or_else(|| RuntimeError::ChartPreviewNotFound(preview_id))
}

async fn list_session_traces(
    State(service): State<RuntimeService>,
    Path(session_id): Path<String>,
) -> RuntimeResult<Json<Vec<RunTraceSummary>>> {
    service.list_run_traces(&session_id).await.map(Json)
}

#[derive(Debug, Deserialize)]
struct UpdateSessionRequest {
    #[serde(default)]
    title: Option<String>,
    #[serde(default)]
    status: Option<quire_session_protocol::SessionStatus>,
    #[serde(default)]
    pinned: Option<bool>,
}

async fn update_session(
    State(service): State<RuntimeService>,
    Path(session_id): Path<String>,
    Json(request): Json<UpdateSessionRequest>,
) -> RuntimeResult<Json<SessionSummary>> {
    service
        .update_session(
            &session_id,
            SessionPatch {
                title: request.title.as_deref(),
                status: request.status,
                pinned: request.pinned,
            },
        )
        .await
        .map(Json)
}

async fn delete_session(
    State(service): State<RuntimeService>,
    Path(session_id): Path<String>,
) -> RuntimeResult<StatusCode> {
    service.delete_session(&session_id).await?;
    Ok(StatusCode::NO_CONTENT)
}

#[derive(Debug, Serialize)]
struct PostMessageResponse {
    #[serde(skip_serializing_if = "Option::is_none")]
    message: Option<quire_session_protocol::SessionMessage>,
    inserted: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    run_id: Option<String>,
    queued: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    queue_item: Option<PendingQueueItem>,
}

async fn post_message(
    State(service): State<RuntimeService>,
    Path(session_id): Path<String>,
    Json(request): Json<SendMessageRequest>,
) -> RuntimeResult<Json<PostMessageResponse>> {
    let outcome = service.post_message(&session_id, &request).await?;
    let response = match outcome {
        PostMessageOutcome::Started { message, run_id } => PostMessageResponse {
            message: Some(message),
            inserted: true,
            run_id: Some(run_id),
            queued: false,
            queue_item: None,
        },
        PostMessageOutcome::Replayed { message } => PostMessageResponse {
            message: Some(message),
            inserted: false,
            run_id: None,
            queued: false,
            queue_item: None,
        },
        PostMessageOutcome::Queued { item } => PostMessageResponse {
            message: None,
            inserted: true,
            run_id: None,
            queued: true,
            queue_item: Some(item),
        },
    };
    Ok(Json(response))
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct SessionCommentCreateRequest {
    block_id: String,
    #[serde(default)]
    end_block_id: Option<String>,
    #[serde(default)]
    anchor_blocks: Vec<String>,
    body: String,
    #[serde(default)]
    quote: Option<TextQuoteSelector>,
    #[serde(default)]
    end_quote: Option<TextQuoteSelector>,
    #[serde(default)]
    anchor_snapshot: Option<String>,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct SessionCommentReplyRequest {
    body: String,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct SessionCommentStatusRequest {
    status: String,
}

// Queue updates use the shared session wire contract.

#[derive(Debug)]
struct CommentProxyError {
    status: StatusCode,
    code: &'static str,
    message: String,
    hint: &'static str,
}

impl IntoResponse for CommentProxyError {
    fn into_response(self) -> Response {
        (
            self.status,
            Json(ApiErrorEnvelope::<()>::new(
                self.code,
                self.message,
                self.hint,
                None,
            )),
        )
            .into_response()
    }
}

fn proxy_runtime_error(error: RuntimeError) -> CommentProxyError {
    let status = match error {
        RuntimeError::SessionNotFound(_) => StatusCode::NOT_FOUND,
        RuntimeError::InvalidRequest(_) => StatusCode::BAD_REQUEST,
        _ => StatusCode::INTERNAL_SERVER_ERROR,
    };
    CommentProxyError {
        status,
        code: error.code(),
        message: error.to_string(),
        hint: error.hint(),
    }
}

fn store_comment_error(error: quire_store::StoreError) -> CommentProxyError {
    let status = match &error {
        quire_store::StoreError::NotFound { .. }
        | quire_store::StoreError::CommentThreadNotFound { .. }
        | quire_store::StoreError::CommentTargetNotFound { .. } => StatusCode::NOT_FOUND,
        quire_store::StoreError::InvalidDocId { .. }
        | quire_store::StoreError::InvalidRequest(_)
        | quire_store::StoreError::Protocol(_) => StatusCode::BAD_REQUEST,
        _ => StatusCode::INTERNAL_SERVER_ERROR,
    };
    CommentProxyError {
        status,
        code: error.code(),
        message: error.to_string(),
        hint: "Inspect the comment payload and the bound report, then retry.",
    }
}

async fn create_session_comment(
    State(service): State<RuntimeService>,
    Path(session_id): Path<String>,
    Json(request): Json<SessionCommentCreateRequest>,
) -> Result<Json<CommentThread>, CommentProxyError> {
    let raw_doc_id = session_comment_doc_id(&service, &session_id)
        .await
        .map_err(proxy_runtime_error)?;
    let doc_id = quire_protocol::DocId::parse(raw_doc_id)
        .map_err(|error| store_comment_error(quire_store::StoreError::Protocol(error)))?;
    let block_id = quire_protocol::BlockId::parse(request.block_id)
        .map_err(|error| store_comment_error(quire_store::StoreError::Protocol(error)))?;
    let end_block_id = request
        .end_block_id
        .map(quire_protocol::BlockId::parse)
        .transpose()
        .map_err(|error| store_comment_error(quire_store::StoreError::Protocol(error)))?;
    let anchor_blocks = request
        .anchor_blocks
        .into_iter()
        .map(quire_protocol::BlockId::parse)
        .collect::<Result<Vec<_>, _>>()
        .map_err(|error| store_comment_error(quire_store::StoreError::Protocol(error)))?;
    if let Some(quote) = &request.quote {
        quote.validate().map_err(|reason| {
            store_comment_error(quire_store::StoreError::InvalidRequest(reason))
        })?;
    }
    if let Some(end_quote) = &request.end_quote {
        end_quote.validate().map_err(|reason| {
            store_comment_error(quire_store::StoreError::InvalidRequest(reason))
        })?;
    }
    let thread = service
        .store()
        .report_store()
        .create_comment_thread(
            &doc_id,
            quire_store::NewCommentThread {
                block_id,
                end_block_id,
                anchor_blocks,
                author_type: quire_protocol::CommentAuthorType::Human,
                author: "reviewer".into(),
                body: request.body,
                quote: request.quote,
                end_quote: request.end_quote,
                anchor_snapshot: request.anchor_snapshot,
            },
        )
        .await
        .map_err(store_comment_error)?;
    service
        .report_app_state()
        .publish_comment_updated(&thread)
        .await;
    let comment_ref = session_comment_ref(&thread);
    if let Err(error) = service.post_comment_message(&session_id, comment_ref).await {
        tracing::warn!(%error, %session_id, thread_id = %thread.thread_id, "failed to auto-trigger comment review");
    }
    Ok(Json(thread))
}

async fn reply_session_comment(
    State(service): State<RuntimeService>,
    Path((session_id, thread_id)): Path<(String, String)>,
    Json(request): Json<SessionCommentReplyRequest>,
) -> Result<Json<CommentThread>, CommentProxyError> {
    let raw_doc_id = session_comment_doc_id(&service, &session_id)
        .await
        .map_err(proxy_runtime_error)?;
    let doc_id = quire_protocol::DocId::parse(raw_doc_id)
        .map_err(|error| store_comment_error(quire_store::StoreError::Protocol(error)))?;
    let thread = service
        .store()
        .report_store()
        .reply_comment_thread(
            &doc_id,
            &thread_id,
            quire_store::NewCommentReply {
                author_type: quire_protocol::CommentAuthorType::Human,
                author: "reviewer".into(),
                body: request.body,
                mark_addressed: false,
                addressed_rev: None,
            },
        )
        .await
        .map_err(store_comment_error)?;
    service
        .report_app_state()
        .publish_comment_updated(&thread)
        .await;
    if thread.status != CommentStatus::Archived
        && let Some(reply) = thread.messages.last()
    {
        let comment_ref = session_comment_ref_with_body(&thread, &reply.body);
        if let Err(error) = service
            .post_comment_reply_message(&session_id, &reply.message_id, comment_ref)
            .await
        {
            tracing::warn!(
                %error,
                %session_id,
                thread_id = %thread.thread_id,
                "failed to auto-trigger agent for a comment follow-up"
            );
        }
    }
    Ok(Json(thread))
}

async fn update_session_comment_status(
    State(service): State<RuntimeService>,
    Path((session_id, thread_id)): Path<(String, String)>,
    Json(request): Json<SessionCommentStatusRequest>,
) -> Result<Json<CommentThread>, CommentProxyError> {
    let raw_doc_id = session_comment_doc_id(&service, &session_id)
        .await
        .map_err(proxy_runtime_error)?;
    let doc_id = quire_protocol::DocId::parse(raw_doc_id)
        .map_err(|error| store_comment_error(quire_store::StoreError::Protocol(error)))?;
    let status = CommentStatus::parse(&request.status).ok_or_else(|| {
        store_comment_error(quire_store::StoreError::InvalidRequest(format!(
            "unknown comment status `{}`; expected open, addressed or archived",
            request.status
        )))
    })?;
    let thread = service
        .store()
        .report_store()
        .update_comment_status(&doc_id, &thread_id, status)
        .await
        .map_err(store_comment_error)?;
    service
        .report_app_state()
        .publish_comment_updated(&thread)
        .await;
    Ok(Json(thread))
}

async fn delete_session_comment(
    State(service): State<RuntimeService>,
    Path((session_id, thread_id)): Path<(String, String)>,
) -> RuntimeResult<StatusCode> {
    let raw_doc_id = session_comment_doc_id(&service, &session_id).await?;
    let doc_id = quire_protocol::DocId::parse(raw_doc_id)
        .map_err(|error| RuntimeError::InvalidRequest(error.to_string()))?;
    service
        .remove_pending_comment_threads(&session_id, &thread_id)
        .await?;
    match service
        .store()
        .report_store()
        .delete_comment_thread(&doc_id, &thread_id)
        .await
    {
        Ok(()) | Err(quire_store::StoreError::CommentThreadNotFound { .. }) => {
            service
                .report_app_state()
                .publish_comment_deleted(&doc_id, &thread_id)
                .await;
            Ok(StatusCode::NO_CONTENT)
        }
        Err(error) => Err(RuntimeError::Internal(error.to_string())),
    }
}

async fn list_session_queue(
    State(service): State<RuntimeService>,
    Path(session_id): Path<String>,
) -> RuntimeResult<Json<Vec<PendingQueueItem>>> {
    service.list_pending_queue(&session_id).await.map(Json)
}

async fn update_session_queue_item(
    State(service): State<RuntimeService>,
    Path((session_id, queue_id)): Path<(String, String)>,
    Json(request): Json<UpdatePendingMessageRequest>,
) -> RuntimeResult<Json<PendingQueueItem>> {
    service
        .update_pending_message(
            &session_id,
            &queue_id,
            &request.content,
            &request.comment_refs,
        )
        .await
        .map(Json)
}

async fn delete_session_queue_item(
    State(service): State<RuntimeService>,
    Path((session_id, queue_id)): Path<(String, String)>,
) -> RuntimeResult<StatusCode> {
    if service
        .delete_pending_message(&session_id, &queue_id)
        .await?
    {
        Ok(StatusCode::NO_CONTENT)
    } else {
        Err(RuntimeError::PendingQueueItemNotFound(queue_id))
    }
}

async fn session_comment_doc_id(
    service: &RuntimeService,
    session_id: &str,
) -> RuntimeResult<String> {
    let session = service.store().get_session_summary(session_id).await?;
    session
        .doc_id
        .filter(|doc_id| !doc_id.trim().is_empty())
        .or_else(|| {
            session
                .linked_doc_id
                .filter(|doc_id| !doc_id.trim().is_empty())
        })
        .ok_or_else(|| {
            RuntimeError::InvalidRequest(format!(
                "session `{session_id}` is not bound to a report; create or link a report before commenting"
            ))
        })
}

fn session_comment_ref(thread: &CommentThread) -> SessionCommentRef {
    let comment = thread
        .messages
        .first()
        .map(|message| message.body.as_str())
        .unwrap_or_default();
    session_comment_ref_with_body(thread, comment)
}

fn session_comment_ref_with_body(thread: &CommentThread, comment: &str) -> SessionCommentRef {
    SessionCommentRef {
        thread_id: thread.thread_id.clone(),
        block_id: thread.block_id.as_str().to_string(),
        end_block_id: thread
            .end_block_id
            .as_ref()
            .map(|block_id| block_id.as_str().to_string()),
        anchor_blocks: thread
            .anchor_blocks
            .iter()
            .map(|block_id| block_id.as_str().to_string())
            .collect(),
        status: thread.status.as_str().to_string(),
        quote: thread.quote.as_ref().map(|quote| quote.exact.clone()),
        end_quote: thread.end_quote.as_ref().map(|quote| quote.exact.clone()),
        anchor_snapshot: thread.anchor_snapshot.clone(),
        comment: comment.to_string(),
    }
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct AnswerQuestionRequest {
    #[serde(default)]
    answers: Vec<QuestionAnswer>,
    #[serde(default)]
    skipped: bool,
}

async fn answer_question(
    State(service): State<RuntimeService>,
    Path((session_id, question_id)): Path<(String, String)>,
    Json(request): Json<AnswerQuestionRequest>,
) -> RuntimeResult<Json<SessionDetail>> {
    service
        .answer_question(&session_id, &question_id, &request.answers, request.skipped)
        .await
        .map(Json)
}

#[derive(Debug, Default, Deserialize)]
struct InterruptRequest {
    #[serde(default)]
    run_id: Option<String>,
}

async fn interrupt(
    State(service): State<RuntimeService>,
    Path(session_id): Path<String>,
    request: Option<Json<InterruptRequest>>,
) -> RuntimeResult<Json<SessionDetail>> {
    let run_id = request.as_ref().and_then(|json| json.0.run_id.as_deref());
    service.interrupt(&session_id, run_id).await.map(Json)
}

async fn retry(
    State(service): State<RuntimeService>,
    Path(session_id): Path<String>,
) -> RuntimeResult<Json<SessionDetail>> {
    service.retry(&session_id).await.map(Json)
}

async fn compact(
    State(service): State<RuntimeService>,
    Path(session_id): Path<String>,
) -> RuntimeResult<Json<SessionDetail>> {
    let task = tokio::spawn(async move { service.compact(&session_id).await });
    task.await
        .map_err(|error| RuntimeError::Internal(format!("compaction task failed: {error}")))?
        .map(Json)
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct AttachmentPresignRequest {
    filename: String,
    mime_type: String,
    size_bytes: u64,
}

#[derive(Debug, Deserialize)]
struct AttachmentUpload {
    #[serde(default)]
    session_id: Option<String>,
    filename: String,
    mime_type: String,
    size_bytes: u64,
    #[serde(default)]
    content_base64: Option<String>,
    #[serde(default)]
    attachment_id: Option<String>,
    #[serde(default)]
    object_key: Option<String>,
}

async fn presign_session_attachment(
    State(service): State<RuntimeService>,
    Path(session_id): Path<String>,
    Json(request): Json<AttachmentPresignRequest>,
) -> RuntimeResult<Json<AttachmentPresign>> {
    service
        .presign_attachment(
            &session_id,
            &request.filename,
            &request.mime_type,
            request.size_bytes,
        )
        .await
        .map(Json)
}

async fn post_attachment(
    State(service): State<RuntimeService>,
    Json(request): Json<AttachmentUpload>,
) -> RuntimeResult<Json<AttachmentMeta>> {
    let session_id = request.session_id.clone().ok_or_else(|| {
        RuntimeError::InvalidRequest("session_id is required for /attachments".into())
    })?;
    attach(&service, &session_id, request).await
}

async fn post_session_attachment(
    State(service): State<RuntimeService>,
    Path(session_id): Path<String>,
    Json(request): Json<AttachmentUpload>,
) -> RuntimeResult<Json<AttachmentMeta>> {
    attach(&service, &session_id, request).await
}

async fn attach(
    service: &RuntimeService,
    session_id: &str,
    request: AttachmentUpload,
) -> RuntimeResult<Json<AttachmentMeta>> {
    service
        .create_attachment(AttachmentWrite {
            session_id,
            attachment_id: request.attachment_id.as_deref(),
            filename: &request.filename,
            mime_type: &request.mime_type,
            size_bytes: request.size_bytes,
            content_base64: request.content_base64.as_deref(),
            object_key: request.object_key.as_deref(),
        })
        .await
        .map(Json)
}

async fn get_tool_call(
    State(service): State<RuntimeService>,
    Path((session_id, tool_call_id)): Path<(String, String)>,
) -> RuntimeResult<Json<ToolCallDetail>> {
    service
        .get_tool_call(&session_id, &tool_call_id)
        .await
        .map(Json)
}

#[derive(Debug, Deserialize)]
struct EventsQuery {
    #[serde(rename = "afterSeq", alias = "after_seq")]
    after_seq: Option<u64>,
}

#[derive(Debug, Deserialize)]
struct PruneEventsRequest {
    keep_events: Option<usize>,
}

#[derive(Debug, Serialize)]
struct PruneEventsResponse {
    deleted: u64,
    min_available_seq: u64,
}

async fn prune_events(
    State(service): State<RuntimeService>,
    Path(session_id): Path<String>,
    Json(request): Json<PruneEventsRequest>,
) -> RuntimeResult<Json<PruneEventsResponse>> {
    let (deleted, min_available_seq) = service
        .store()
        .prune_session_events(&session_id, request.keep_events.unwrap_or(1_000))
        .await?;
    Ok(Json(PruneEventsResponse {
        deleted,
        min_available_seq,
    }))
}

async fn session_events(
    State(service): State<RuntimeService>,
    Path(session_id): Path<String>,
    Query(query): Query<EventsQuery>,
) -> Result<Sse<impl Stream<Item = Result<Event, Infallible>>>, RuntimeError> {
    let summary = service.store().get_session_summary(&session_id).await?;
    let min_available_seq = service.store().min_available_seq(&session_id).await?;
    let after_seq = query.after_seq.unwrap_or(0);
    let stream = event_stream(
        service,
        session_id,
        after_seq,
        summary.last_event_seq,
        min_available_seq,
    );
    Ok(Sse::new(stream).keep_alive(KeepAlive::default()))
}

fn event_stream(
    service: RuntimeService,
    session_id: String,
    after_seq: u64,
    server_last_seq: u64,
    min_available_seq: u64,
) -> impl Stream<Item = Result<Event, Infallible>> {
    async_stream::stream! {
        yield Ok(Event::default().comment("connected"));
        if after_seq > server_last_seq {
            yield Ok(sse_event(&synthetic_sync(
                &session_id,
                after_seq,
                "client cursor is ahead of the server",
            )));
            return;
        }
        if after_seq.saturating_add(1) < min_available_seq {
            yield Ok(sse_event(&synthetic_sync(
                &session_id,
                after_seq,
                "replay cursor is older than the retained event window",
            )));
            return;
        }
        let store = service.store().clone();
        let bus = store.bus().clone();
        let replay_limit = service.config().sse_replay_limit;
        let total = store.count_events_after(&session_id, after_seq).await.unwrap_or(0);
        if u64::try_from(total).unwrap_or(u64::MAX) > u64::try_from(replay_limit).unwrap_or(u64::MAX) {
            yield Ok(sse_event(&synthetic_sync(&session_id, after_seq, "replay window exceeded")));
            return;
        }
        let mut subscriber = match bus.subscribe_events(&session_id).await {
            Ok(subscriber) => subscriber,
            Err(error) => {
                yield Ok(sse_event(&synthetic_sync(
                    &session_id,
                    after_seq,
                    &format!("live subscription unavailable: {error}"),
                )));
                return;
            }
        };
        let mut last_seq = after_seq;
        match store.list_event_frames(&session_id, after_seq, i64::try_from(replay_limit).unwrap_or(500)).await {
            Ok(frames) => {
                for frame in frames {
                    last_seq = frame.seq;
                    yield Ok(sse_frame(&frame));
                }
            }
            Err(error) => {
                yield Ok(sse_event(&synthetic_sync(
                    &session_id,
                    last_seq,
                    &format!("replay failed: {error}"),
                )));
                return;
            }
        }
        let mut heartbeat = tokio::time::interval(Duration::from_millis(service.config().sse_heartbeat_ms));
        loop {
            tokio::select! {
                event = subscriber.next_event() => {
                    let Some(event) = event else { break };
                    if event.seq <= last_seq {
                        continue;
                    }
                    if event.seq > last_seq.saturating_add(1) {
                        let gap = event.seq.saturating_sub(last_seq);
                        let replay_window = u64::try_from(replay_limit).unwrap_or(u64::MAX);
                        if gap > replay_window {
                            yield Ok(sse_event(&synthetic_sync(
                                &session_id,
                                last_seq,
                                "live event gap exceeded the replay window",
                            )));
                            break;
                        }
                        let limit = i64::try_from(gap).unwrap_or(i64::MAX);
                        match store.list_event_frames(&session_id, last_seq, limit).await {
                            Ok(frames) => {
                                for frame in frames {
                                    if frame.seq <= last_seq {
                                        continue;
                                    }
                                    if frame.seq > event.seq {
                                        break;
                                    }
                                    last_seq = frame.seq;
                                    yield Ok(sse_frame(&frame));
                                }
                            }
                            Err(error) => {
                                yield Ok(sse_event(&synthetic_sync(
                                    &session_id,
                                    last_seq,
                                    &format!("gap replay failed: {error}"),
                                )));
                                break;
                            }
                        }
                        if event.seq <= last_seq {
                            continue;
                        }
                        if event.seq > last_seq.saturating_add(1) {
                            yield Ok(sse_event(&synthetic_sync(
                                &session_id,
                                last_seq,
                                "live event gap could not be fully replayed",
                            )));
                            break;
                        }
                    }
                    last_seq = event.seq;
                    yield Ok(sse_event(&event));
                }
                _ = heartbeat.tick() => {
                    // Pub/Sub is only a wake-up signal. Reconcile with the durable
                    // log so a lost terminal publish is repaired without a reconnect.
                    match store.get_session_summary(&session_id).await {
                        Ok(summary) if summary.last_event_seq > last_seq => {
                            let gap = summary.last_event_seq.saturating_sub(last_seq);
                            let replay_window = u64::try_from(replay_limit).unwrap_or(u64::MAX);
                            if gap > replay_window {
                                yield Ok(sse_event(&synthetic_sync(
                                    &session_id,
                                    last_seq,
                                    "reconcile gap exceeded the replay window",
                                )));
                                break;
                            }
                            let limit = i64::try_from(gap).unwrap_or(i64::MAX);
                            match store.list_event_frames(&session_id, last_seq, limit).await {
                                Ok(frames) => {
                                    for frame in frames {
                                        if frame.seq <= last_seq {
                                            continue;
                                        }
                                        last_seq = frame.seq;
                                        yield Ok(sse_frame(&frame));
                                    }
                                }
                                Err(error) => {
                                    yield Ok(sse_event(&synthetic_sync(
                                        &session_id,
                                        last_seq,
                                        &format!("reconcile replay failed: {error}"),
                                    )));
                                    break;
                                }
                            }
                        }
                        Ok(_) => {}
                        Err(error) => {
                            warn!(%error, %session_id, "session SSE reconcile failed; retrying on the next heartbeat");
                        }
                    }
                    yield Ok(Event::default().comment("heartbeat"));
                }
            }
        }
    }
}

fn sse_frame(frame: &SessionEventFrame) -> Event {
    Event::default()
        .event(frame.event_type.as_str())
        .json_data(&frame.event)
        .unwrap_or_else(|error| {
            Event::default()
                .event("error")
                .data(format!("serialization error: {error}"))
        })
}

fn synthetic_sync(session_id: &str, after_seq: u64, reason: &str) -> SessionEvent {
    let _ = session_id;
    SessionEvent::new(
        after_seq,
        "system",
        "system",
        None,
        SessionEventKind::SyncRequired {
            after_seq,
            reason: reason.to_string(),
        },
    )
}

fn sse_event(event: &SessionEvent) -> Event {
    Event::default()
        .event(event.event_name())
        .json_data(event)
        .unwrap_or_else(|error| {
            Event::default()
                .event("error")
                .data(format!("serialization error: {error}"))
        })
}

// ---------------------------------------------------------------------------
// Model/provider registry
// ---------------------------------------------------------------------------

async fn list_model_providers(
    State(service): State<RuntimeService>,
) -> RuntimeResult<Json<Vec<ProviderRecord>>> {
    service.models().list_providers().await.map(Json)
}

async fn create_model_provider(
    State(service): State<RuntimeService>,
    Json(input): Json<ProviderInput>,
) -> RuntimeResult<Json<ProviderRecord>> {
    service.models().upsert_provider(input).await.map(Json)
}

async fn update_model_provider(
    State(service): State<RuntimeService>,
    Path(provider_id): Path<String>,
    Json(mut input): Json<ProviderInput>,
) -> RuntimeResult<Json<ProviderRecord>> {
    input.provider_id = provider_id;
    service.models().upsert_provider(input).await.map(Json)
}

async fn delete_model_provider(
    State(service): State<RuntimeService>,
    Path(provider_id): Path<String>,
) -> RuntimeResult<StatusCode> {
    service.models().delete_provider(&provider_id).await?;
    Ok(StatusCode::NO_CONTENT)
}

async fn list_models(
    State(service): State<RuntimeService>,
) -> RuntimeResult<Json<Vec<ModelRecord>>> {
    service.models().list_models().await.map(Json)
}

async fn create_model(
    State(service): State<RuntimeService>,
    Json(input): Json<ModelInput>,
) -> RuntimeResult<Json<ModelRecord>> {
    service.models().upsert_model(input).await.map(Json)
}

async fn update_model(
    State(service): State<RuntimeService>,
    Path(id): Path<String>,
    Json(mut input): Json<ModelInput>,
) -> RuntimeResult<Json<ModelRecord>> {
    input.id = id;
    service.models().upsert_model(input).await.map(Json)
}

async fn delete_model(
    State(service): State<RuntimeService>,
    Path(id): Path<String>,
) -> RuntimeResult<StatusCode> {
    service.models().delete_model(&id).await?;
    Ok(StatusCode::NO_CONTENT)
}

async fn import_models_dev(
    State(service): State<RuntimeService>,
    Json(request): Json<ModelsDevImportRequest>,
) -> RuntimeResult<Json<ModelsDevImportReport>> {
    service.models().import_models_dev(request).await.map(Json)
}

// ---------------------------------------------------------------------------
// Global MCP registry
// ---------------------------------------------------------------------------

#[derive(Debug, Default, Deserialize)]
#[serde(deny_unknown_fields)]
struct McpToolsQuery {
    #[serde(default)]
    server_id: Option<String>,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct McpToolToggleRequest {
    enabled: bool,
}

async fn list_mcp_servers(
    State(service): State<RuntimeService>,
) -> RuntimeResult<Json<Vec<McpServerRecord>>> {
    service.mcp().list_servers().await.map(Json)
}

async fn upsert_mcp_server(
    State(service): State<RuntimeService>,
    Json(input): Json<McpServerInput>,
) -> RuntimeResult<Json<McpServerRecord>> {
    service.mcp().upsert_server(input).await.map(Json)
}

async fn patch_mcp_server(
    State(service): State<RuntimeService>,
    Path(server_id): Path<String>,
    Json(patch): Json<McpServerPatch>,
) -> RuntimeResult<Json<McpServerRecord>> {
    service
        .mcp()
        .patch_server(&server_id, patch)
        .await
        .map(Json)
}

async fn delete_mcp_server(
    State(service): State<RuntimeService>,
    Path(server_id): Path<String>,
) -> RuntimeResult<StatusCode> {
    service.mcp().delete_server(&server_id).await?;
    Ok(StatusCode::NO_CONTENT)
}

async fn sync_mcp_server(
    State(service): State<RuntimeService>,
    Path(server_id): Path<String>,
) -> RuntimeResult<Json<McpSyncReport>> {
    service.mcp().sync_server_tools(&server_id).await.map(Json)
}

async fn list_mcp_tools(
    State(service): State<RuntimeService>,
    Query(query): Query<McpToolsQuery>,
) -> RuntimeResult<Json<Vec<McpToolRecord>>> {
    service
        .mcp()
        .list_tools(query.server_id.as_deref())
        .await
        .map(Json)
}

async fn set_mcp_tool_enabled(
    State(service): State<RuntimeService>,
    Path((server_id, tool_name)): Path<(String, String)>,
    Json(request): Json<McpToolToggleRequest>,
) -> RuntimeResult<Json<McpToolRecord>> {
    service
        .mcp()
        .set_tool_enabled(&server_id, &tool_name, request.enabled)
        .await
        .map(Json)
}

// ---------------------------------------------------------------------------
// Knowledge-base CRUD
// ---------------------------------------------------------------------------

#[derive(Debug, Deserialize)]
struct CreateDocumentRequest {
    doc_id: String,
    name: String,
    content: String,
}

#[derive(Debug, Deserialize)]
struct UpdateDocumentRequest {
    name: String,
    content: String,
}

async fn list_kb_docs(
    State(service): State<RuntimeService>,
) -> RuntimeResult<Json<Vec<KbDocumentRecord>>> {
    service.kb().list_documents().await.map(Json)
}

async fn create_kb_doc(
    State(service): State<RuntimeService>,
    Json(request): Json<CreateDocumentRequest>,
) -> RuntimeResult<Json<KbDocumentRecord>> {
    service
        .kb()
        .upsert_document(&request.doc_id, &request.name, &request.content)
        .await
        .map(Json)
}

async fn update_kb_doc(
    State(service): State<RuntimeService>,
    Path(doc_id): Path<String>,
    Json(request): Json<UpdateDocumentRequest>,
) -> RuntimeResult<Json<KbDocumentRecord>> {
    service
        .kb()
        .upsert_document(&doc_id, &request.name, &request.content)
        .await
        .map(Json)
}

async fn delete_kb_doc(
    State(service): State<RuntimeService>,
    Path(doc_id): Path<String>,
) -> RuntimeResult<StatusCode> {
    service.kb().delete_document(&doc_id).await?;
    Ok(StatusCode::NO_CONTENT)
}

#[derive(Debug, Deserialize)]
struct AgentRequest {
    #[serde(default)]
    name: Option<String>,
    #[serde(default)]
    description: String,
    model: String,
    system_prompt: String,
    #[serde(default)]
    allowed_tools: Vec<String>,
    #[serde(default)]
    is_default: bool,
    #[serde(default = "default_enabled")]
    enabled: bool,
}

fn default_enabled() -> bool {
    true
}

async fn list_agents(
    State(service): State<RuntimeService>,
) -> RuntimeResult<Json<Vec<AgentDefinitionRecord>>> {
    service.kb().list_agents().await.map(Json)
}

async fn create_agent(
    State(service): State<RuntimeService>,
    Json(request): Json<AgentRequest>,
) -> RuntimeResult<Json<AgentDefinitionRecord>> {
    let name = request
        .name
        .ok_or_else(|| RuntimeError::InvalidRequest("agent name is required".into()))?;
    service
        .kb()
        .upsert_agent(UpsertKbAgentCommand {
            name: &name,
            description: &request.description,
            model: &request.model,
            system_prompt: &request.system_prompt,
            allowed_tools: &request.allowed_tools,
            is_default: request.is_default,
            enabled: request.enabled,
        })
        .await
        .map(Json)
}

async fn update_agent(
    State(service): State<RuntimeService>,
    Path(name): Path<String>,
    Json(request): Json<AgentRequest>,
) -> RuntimeResult<Json<AgentDefinitionRecord>> {
    service
        .kb()
        .upsert_agent(UpsertKbAgentCommand {
            name: &name,
            description: &request.description,
            model: &request.model,
            system_prompt: &request.system_prompt,
            allowed_tools: &request.allowed_tools,
            is_default: request.is_default,
            enabled: request.enabled,
        })
        .await
        .map(Json)
}

async fn delete_agent(
    State(service): State<RuntimeService>,
    Path(name): Path<String>,
) -> RuntimeResult<StatusCode> {
    service.kb().delete_agent(&name).await?;
    Ok(StatusCode::NO_CONTENT)
}

#[derive(Debug, Deserialize)]
struct ResourceInput {
    path: String,
    content: String,
}

#[derive(Debug, Deserialize)]
struct SkillRequest {
    #[serde(default)]
    name: Option<String>,
    description: String,
    body: String,
    #[serde(default = "default_enabled")]
    enabled: bool,
    #[serde(default)]
    resources: Vec<ResourceInput>,
}

async fn list_skills(
    State(service): State<RuntimeService>,
) -> RuntimeResult<Json<Vec<SkillDefinitionRecord>>> {
    service.kb().list_skills().await.map(Json)
}

async fn create_skill(
    State(service): State<RuntimeService>,
    Json(request): Json<SkillRequest>,
) -> RuntimeResult<Json<SkillDefinitionRecord>> {
    let name = request
        .name
        .clone()
        .ok_or_else(|| RuntimeError::InvalidRequest("skill name is required".into()))?;
    upsert_skill(&service, &name, request).await
}

async fn update_skill(
    State(service): State<RuntimeService>,
    Path(name): Path<String>,
    Json(request): Json<SkillRequest>,
) -> RuntimeResult<Json<SkillDefinitionRecord>> {
    upsert_skill(&service, &name, request).await
}

async fn upsert_skill(
    service: &RuntimeService,
    name: &str,
    request: SkillRequest,
) -> RuntimeResult<Json<SkillDefinitionRecord>> {
    let resources: Vec<(String, String)> = request
        .resources
        .into_iter()
        .map(|resource| (resource.path, resource.content))
        .collect();
    service
        .kb()
        .upsert_skill(
            name,
            &request.description,
            &request.body,
            request.enabled,
            &resources,
        )
        .await
        .map(Json)
}

async fn delete_skill(
    State(service): State<RuntimeService>,
    Path(name): Path<String>,
) -> RuntimeResult<StatusCode> {
    service.kb().delete_skill(&name).await?;
    Ok(StatusCode::NO_CONTENT)
}

// ---------------------------------------------------------------------------
// Knowledge-base branches, promotion and rollback
// ---------------------------------------------------------------------------

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct CreateBranchRequest {
    name: String,
    #[serde(default)]
    description: String,
    #[serde(default, alias = "base", alias = "source", alias = "from_branch")]
    from: Option<String>,
}

async fn list_kb_branches(
    State(service): State<RuntimeService>,
) -> RuntimeResult<Json<Vec<KbBranchRecord>>> {
    service.kb().list_branches().await.map(Json)
}

async fn create_kb_branch(
    State(service): State<RuntimeService>,
    Json(request): Json<CreateBranchRequest>,
) -> RuntimeResult<Json<KbBranchRecord>> {
    service
        .kb()
        .create_branch(&request.name, &request.description, request.from.as_deref())
        .await
        .map(Json)
}

async fn diff_kb_branch(
    State(service): State<RuntimeService>,
    Path(name): Path<String>,
) -> RuntimeResult<Json<KbBranchDiff>> {
    service.kb().diff_branch(&name).await.map(Json)
}

async fn promote_kb_branch(
    State(service): State<RuntimeService>,
    Path(name): Path<String>,
) -> RuntimeResult<Json<KbPromoteReport>> {
    service.kb().promote_branch(&name).await.map(Json)
}

#[derive(Debug, Default, Deserialize)]
#[serde(deny_unknown_fields)]
struct RollbackRequest {
    #[serde(default)]
    version: Option<u64>,
}

async fn rollback_kb(
    State(service): State<RuntimeService>,
    request: Option<Json<RollbackRequest>>,
) -> RuntimeResult<Json<KbRollbackReport>> {
    let version = request.as_ref().and_then(|request| request.0.version);
    service.kb().rollback_active(version).await.map(Json)
}

/// Reserved for attachment IDs generated by clients before upload.
pub fn new_attachment_id() -> String {
    short_id("att")
}
