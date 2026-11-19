//! Axum REST and SSE endpoints for the read-only frontend.

use axum::{
    Json, Router,
    body::Bytes,
    extract::{Path, Query, State},
    http::{HeaderValue, StatusCode},
    middleware,
    response::{
        IntoResponse, Response,
        sse::{Event, KeepAlive, Sse},
    },
    routing::{get, patch, post},
};
use chrono::Utc;
use futures::Stream;
use quire_protocol::{
    BlockId, CommentAuthorType, CommentStatus, CommentThread, ComputedDocumentMeta,
    DocumentDiffReport, EphemeralPayload, ReportDocument, TextQuoteSelector, compute_document_meta,
    diff_documents,
};
use quire_protocol::{DocId, Revision, SessionId};
use quire_store::{
    NewCommentReply, NewCommentThread, ReportList, RevisionRecord, RollbackResult, StoreError,
};
use rmcp::transport::streamable_http_server::{
    StreamableHttpServerConfig, StreamableHttpService, session::local::LocalSessionManager,
};
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use tokio_util::sync::CancellationToken;
use tower_http::{
    cors::{Any, CorsLayer},
    trace::TraceLayer,
};

use crate::{
    auth::{self, RequestContext},
    error::ApiError,
    mcp::QuireMcpServer,
    sse,
    state::AppState,
};

pub fn build_router(state: AppState) -> Router {
    let cancellation = CancellationToken::new();
    let mcp_config = StreamableHttpServerConfig::default()
        .with_allowed_hosts(state.config.allowed_hosts.clone())
        .with_json_response(state.config.mcp_json_response)
        .with_sse_keep_alive(Some(std::time::Duration::from_secs(15)))
        .with_cancellation_token(cancellation);
    let mcp_state = state.clone();
    let mcp_service: StreamableHttpService<QuireMcpServer, LocalSessionManager> =
        StreamableHttpService::new(
            move || Ok(QuireMcpServer::new(mcp_state.clone())),
            Default::default(),
            mcp_config,
        );

    let read_only = Router::new()
        .route("/api/docs", get(list_documents))
        .route("/api/docs/{doc_id}", get(get_document))
        .route("/api/docs/{doc_id}/events", get(document_events))
        .route("/api/docs/{doc_id}/revisions", get(list_revisions))
        .route("/api/docs/{doc_id}/comments", get(list_comments))
        .route("/api/docs/{doc_id}/diff", get(document_diff))
        .route("/api/sessions/{session_id}/events", get(session_events))
        .route_layer(middleware::from_fn_with_state(state.clone(), auth::ro_auth));

    let read_write = Router::new()
        .route("/api/docs/{doc_id}/rollback", post(rollback))
        .route("/api/docs/{doc_id}/ephemeral", post(ephemeral))
        .route("/api/docs/{doc_id}/comments", post(create_comment))
        .route(
            "/api/docs/{doc_id}/comments/{thread_id}/replies",
            post(reply_comment),
        )
        .route(
            "/api/docs/{doc_id}/comments/{thread_id}",
            patch(update_comment_status).delete(delete_comment),
        )
        .route_layer(middleware::from_fn_with_state(state.clone(), auth::rw_auth));

    let mcp_router = Router::new()
        .nest_service("/mcp", mcp_service)
        .route_layer(middleware::from_fn_with_state(state.clone(), auth::rw_auth));

    Router::new()
        .route("/health", get(health))
        .merge(read_only)
        .merge(read_write)
        .merge(mcp_router)
        .with_state(state.clone())
        .layer(cors_layer(&state))
        .layer(TraceLayer::new_for_http())
}

fn cors_layer(state: &AppState) -> CorsLayer {
    let base = CorsLayer::new()
        .allow_methods([
            axum::http::Method::GET,
            axum::http::Method::POST,
            axum::http::Method::PATCH,
            axum::http::Method::DELETE,
            axum::http::Method::OPTIONS,
        ])
        .allow_headers(Any);
    if state.config.cors_allowed_origins.is_empty() {
        base.allow_origin(Any)
    } else {
        let origins: Vec<HeaderValue> = state
            .config
            .cors_allowed_origins
            .iter()
            .filter_map(|origin| origin.parse::<HeaderValue>().ok())
            .collect();
        base.allow_origin(origins)
    }
}

async fn health(State(state): State<AppState>) -> Response {
    match state.store.health().await {
        Ok(()) => (
            StatusCode::OK,
            Json(json!({
                "status": "ok",
                "service": "quire",
                "version": env!("CARGO_PKG_VERSION"),
                "database": "ok",
                "redis": "ok"
            })),
        )
            .into_response(),
        Err(error) => (
            StatusCode::SERVICE_UNAVAILABLE,
            Json(json!({
                "status": "degraded",
                "service": "quire",
                "version": env!("CARGO_PKG_VERSION"),
                "error": error.to_string(),
            })),
        )
            .into_response(),
    }
}

#[derive(Debug, Deserialize)]
struct ListQuery {
    query: Option<String>,
    limit: Option<i64>,
    offset: Option<i64>,
    page: Option<i64>,
    page_size: Option<i64>,
}

async fn list_documents(
    State(state): State<AppState>,
    Query(query): Query<ListQuery>,
) -> Result<Json<ReportList>, ApiError> {
    let page_size = query.page_size.or(query.limit).unwrap_or(20).clamp(1, 100);
    let offset = query
        .offset
        .or_else(|| query.page.map(|page| (page.max(1) - 1) * page_size))
        .unwrap_or(0)
        .max(0);
    let mut list = state
        .store
        .list_reports(query.query.as_deref(), page_size, offset)
        .await?;
    list.limit = page_size;
    list.offset = offset;
    Ok(Json(list))
}

#[derive(Debug, Serialize)]
struct DocumentView {
    #[serde(flatten)]
    document: ReportDocument,
    current_rev: Revision,
    computed: ComputedDocumentMeta,
}

async fn get_document(
    State(state): State<AppState>,
    Path(doc_id): Path<DocId>,
) -> Result<Json<DocumentView>, ApiError> {
    let loaded = state.store.load_document(&doc_id).await?;
    let computed = compute_document_meta(&loaded.document);
    let current_rev = loaded.document.revision;
    Ok(Json(DocumentView {
        current_rev,
        document: loaded.document,
        computed,
    }))
}

#[derive(Debug, Deserialize)]
struct EventsQuery {
    #[serde(rename = "sinceRev", alias = "since_rev")]
    since_rev: Option<i64>,
}

async fn document_events(
    State(state): State<AppState>,
    Path(doc_id): Path<DocId>,
    Query(query): Query<EventsQuery>,
) -> Result<Sse<impl Stream<Item = Result<Event, std::convert::Infallible>>>, ApiError> {
    let since_rev = Revision::new(query.since_rev.unwrap_or(0).max(0))?;
    let document = state.store.get_document(&doc_id).await?;
    let current_rev = document.revision;
    if since_rev > current_rev {
        return Err(ApiError(StoreError::InvalidRequest(format!(
            "sinceRev {} is ahead of current revision {}",
            since_rev.get(),
            current_rev.get()
        ))));
    }
    let stream = sse::revision_stream(state, doc_id, since_rev, current_rev);
    Ok(Sse::new(stream).keep_alive(KeepAlive::default()))
}

async fn session_events(
    State(state): State<AppState>,
    Path(session_id): Path<SessionId>,
) -> Result<Sse<impl Stream<Item = Result<Event, std::convert::Infallible>>>, ApiError> {
    let stream = sse::session_stream(state, session_id);
    Ok(Sse::new(stream).keep_alive(KeepAlive::default()))
}

#[derive(Debug, Deserialize)]
struct RevisionsQuery {
    limit: Option<i64>,
}

#[derive(Debug, Serialize)]
struct RevisionListView {
    doc_id: DocId,
    current_rev: Revision,
    revisions: Vec<RevisionRecord>,
}

async fn list_revisions(
    State(state): State<AppState>,
    Path(doc_id): Path<DocId>,
    Query(query): Query<RevisionsQuery>,
) -> Result<Json<RevisionListView>, ApiError> {
    let document = state.store.get_document(&doc_id).await?;
    let revisions = state
        .store
        .list_revisions(&doc_id, query.limit.unwrap_or(50))
        .await?;
    Ok(Json(RevisionListView {
        doc_id,
        current_rev: document.revision,
        revisions,
    }))
}

#[derive(Debug, Deserialize)]
struct RollbackRequest {
    target_revision: i64,
    #[serde(default)]
    actor: Option<String>,
}

#[derive(Debug, Serialize)]
struct RollbackView {
    #[serde(flatten)]
    result: RollbackResult,
    current_rev: Revision,
}

async fn rollback(
    State(state): State<AppState>,
    Path(doc_id): Path<DocId>,
    context: RequestContext,
    Json(request): Json<RollbackRequest>,
) -> Result<Json<RollbackView>, ApiError> {
    let actor = request
        .actor
        .as_deref()
        .map(str::trim)
        .filter(|actor| !actor.is_empty())
        .unwrap_or(context.actor.as_str());
    let target_revision = Revision::new(request.target_revision)?;
    let result = state
        .store
        .rollback_to_revision(&doc_id, target_revision, actor)
        .await?;
    Ok(Json(RollbackView {
        current_rev: result.new_revision,
        result,
    }))
}

const MAX_EPHEMERAL_BYTES: usize = 64 * 1024;

#[derive(Debug, Serialize)]
struct CommentListView {
    doc_id: DocId,
    comments: Vec<CommentThread>,
}

#[derive(Debug, Deserialize)]
struct CommentListQuery {
    status: Option<String>,
    block_id: Option<String>,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct CreateCommentRequest {
    block_id: String,
    body: String,
    #[serde(default)]
    quote: Option<TextQuoteSelector>,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct ReplyCommentRequest {
    body: String,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct UpdateCommentStatusRequest {
    status: String,
}

fn parse_comment_status(value: &str) -> Result<CommentStatus, ApiError> {
    CommentStatus::parse(value).ok_or_else(|| {
        ApiError(StoreError::InvalidRequest(format!(
            "unknown comment status `{value}`; expected open, addressed, archived or all"
        )))
    })
}

async fn list_comments(
    State(state): State<AppState>,
    Path(doc_id): Path<DocId>,
    Query(query): Query<CommentListQuery>,
) -> Result<Json<CommentListView>, ApiError> {
    let status = match query.status.as_deref() {
        None | Some("all") => None,
        Some(raw) => Some(parse_comment_status(raw)?),
    };
    let block_id = query.block_id.as_deref().map(BlockId::parse).transpose()?;
    let mut comments = state
        .store
        .list_comment_threads(&doc_id, status, block_id.as_ref())
        .await?;
    if query.status.is_none() {
        comments.retain(|thread| thread.status != CommentStatus::Archived);
    }
    Ok(Json(CommentListView { doc_id, comments }))
}

async fn create_comment(
    State(state): State<AppState>,
    Path(doc_id): Path<DocId>,
    context: RequestContext,
    Json(request): Json<CreateCommentRequest>,
) -> Result<Json<CommentThread>, ApiError> {
    let block_id = BlockId::parse(request.block_id)?;
    if let Some(quote) = &request.quote {
        quote
            .validate()
            .map_err(|reason| ApiError(StoreError::InvalidRequest(reason)))?;
    }
    let thread = state
        .store
        .create_comment_thread(
            &doc_id,
            NewCommentThread {
                block_id,
                author_type: CommentAuthorType::Human,
                author: context.actor,
                body: request.body,
                quote: request.quote,
            },
        )
        .await?;
    state.publish_comment_updated(&thread).await;
    Ok(Json(thread))
}

async fn reply_comment(
    State(state): State<AppState>,
    Path((doc_id, thread_id)): Path<(DocId, String)>,
    context: RequestContext,
    Json(request): Json<ReplyCommentRequest>,
) -> Result<Json<CommentThread>, ApiError> {
    let thread = state
        .store
        .reply_comment_thread(
            &doc_id,
            &thread_id,
            NewCommentReply {
                author_type: CommentAuthorType::Human,
                author: context.actor,
                body: request.body,
                mark_addressed: false,
                addressed_rev: None,
            },
        )
        .await?;
    state.publish_comment_updated(&thread).await;
    Ok(Json(thread))
}

async fn update_comment_status(
    State(state): State<AppState>,
    Path((doc_id, thread_id)): Path<(DocId, String)>,
    Json(request): Json<UpdateCommentStatusRequest>,
) -> Result<Json<CommentThread>, ApiError> {
    let status = parse_comment_status(&request.status)?;
    let thread = state
        .store
        .update_comment_status(&doc_id, &thread_id, status)
        .await?;
    state.publish_comment_updated(&thread).await;
    Ok(Json(thread))
}

async fn delete_comment(
    State(state): State<AppState>,
    Path((doc_id, thread_id)): Path<(DocId, String)>,
) -> Result<StatusCode, ApiError> {
    state
        .store
        .delete_comment_thread(&doc_id, &thread_id)
        .await?;
    state.publish_comment_deleted(&doc_id, &thread_id).await;
    Ok(StatusCode::NO_CONTENT)
}

#[derive(Debug, Deserialize)]
struct DiffQuery {
    #[serde(rename = "fromRev", alias = "from_rev")]
    from_rev: i64,
    #[serde(rename = "toRev", alias = "to_rev")]
    to_rev: Option<i64>,
}

async fn document_diff(
    State(state): State<AppState>,
    Path(doc_id): Path<DocId>,
    Query(query): Query<DiffQuery>,
) -> Result<Json<DocumentDiffReport>, ApiError> {
    let from_revision = Revision::new(query.from_rev)?;
    let current = state.store.get_document(&doc_id).await?.revision;
    let to_revision = Revision::new(query.to_rev.unwrap_or(current.get()))?;
    if from_revision > current || to_revision > current {
        return Err(ApiError(StoreError::InvalidRequest(format!(
            "diff revisions must be between 0 and current_rev {}",
            current.get()
        ))));
    }
    let before = state
        .store
        .document_at_revision(&doc_id, from_revision)
        .await?;
    let after = state
        .store
        .document_at_revision(&doc_id, to_revision)
        .await?;
    Ok(Json(diff_documents(&before, &after)))
}

fn parse_ephemeral_body(doc_id: &DocId, body: &[u8]) -> Result<EphemeralPayload, StoreError> {
    let mut value: Value = serde_json::from_slice(body)
        .map_err(|error| StoreError::InvalidRequest(format!("invalid ephemeral JSON: {error}")))?;
    let Some(object) = value.as_object_mut() else {
        return Err(StoreError::InvalidRequest(
            "ephemeral payload must be a JSON object".into(),
        ));
    };
    let received_at = Utc::now();
    // Routing fields are server-owned: overwrite producer values before typed deserialization so
    // clients may omit them without weakening the contract.
    object.insert("doc_id".into(), Value::String(doc_id.to_string()));
    object.insert(
        "received_at".into(),
        Value::String(received_at.to_rfc3339()),
    );
    let mut payload: EphemeralPayload = serde_json::from_value(value).map_err(|error| {
        StoreError::InvalidRequest(format!("invalid ephemeral payload: {error}"))
    })?;
    payload.stamp_server_context(doc_id, received_at);
    payload.validate().map_err(StoreError::InvalidRequest)?;
    Ok(payload)
}

async fn ephemeral(
    State(state): State<AppState>,
    Path(doc_id): Path<DocId>,
    body: Bytes,
) -> Result<(StatusCode, Json<Value>), ApiError> {
    if body.len() > MAX_EPHEMERAL_BYTES {
        return Err(ApiError(StoreError::InvalidRequest(format!(
            "ephemeral payload is {} bytes; limit is {MAX_EPHEMERAL_BYTES}",
            body.len()
        ))));
    }
    let payload = parse_ephemeral_body(&doc_id, &body).map_err(ApiError)?;
    let serialized_len = serde_json::to_vec(&payload)
        .map_err(StoreError::from)?
        .len();
    if serialized_len > MAX_EPHEMERAL_BYTES {
        return Err(ApiError(StoreError::InvalidRequest(format!(
            "ephemeral payload is {serialized_len} bytes after server stamping; limit is {MAX_EPHEMERAL_BYTES}"
        ))));
    }
    state
        .store
        .bus()
        .publish_ephemeral(&doc_id, &payload)
        .await?;
    Ok((
        StatusCode::ACCEPTED,
        Json(json!({
            "published": true,
            "event": payload.event_name(),
            "doc_id": doc_id,
        })),
    ))
}
