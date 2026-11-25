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
    ComputedDocumentMeta, DocumentDiffReport, DocumentSyncDelta, EphemeralPayload, ReportDocument,
    compute_document_meta, diff_documents, sync_documents,
};
use quire_protocol::{DocId, Revision, SessionId};
use quire_store::{ReportList, RevisionRecord, RollbackResult, StoreError};
use rmcp::transport::streamable_http_server::{
    StreamableHttpServerConfig, StreamableHttpService, session::local::LocalSessionManager,
};
use serde::{Deserialize, Serialize};
use serde_json::Value;
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

mod comments;
use comments::{
    create_comment, delete_comment, list_comments, reply_comment, update_comment_status,
};

/// Build the composable document REST/SSE + `/mcp` router so it can be mounted
/// either inside the unified `quire-runtime` server or standalone in `quire-server`.
pub fn build_document_router(state: AppState) -> Router {
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
        .route("/api/docs/{doc_id}/sync", get(document_sync))
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
        .merge(read_only)
        .merge(read_write)
        .merge(mcp_router)
        .with_state(state)
}

pub fn build_router(state: AppState) -> Router {
    let legacy_session_sse = Router::new()
        .route("/api/sessions/{session_id}/events", get(session_events))
        .route_layer(middleware::from_fn_with_state(state.clone(), auth::ro_auth))
        .with_state(state.clone());

    Router::new()
        .route("/health", get(health))
        .with_state(state.clone())
        .merge(legacy_session_sse)
        .merge(build_document_router(state.clone()))
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

#[derive(Serialize)]
struct HealthResponse {
    status: &'static str,
    service: &'static str,
    version: &'static str,
    #[serde(skip_serializing_if = "Option::is_none")]
    error: Option<String>,
}

async fn health(State(state): State<AppState>) -> Response {
    match state.store.health().await {
        Ok(()) => (
            StatusCode::OK,
            Json(HealthResponse {
                status: "ok",
                service: "quire",
                version: env!("CARGO_PKG_VERSION"),
                error: None,
            }),
        )
            .into_response(),
        Err(error) => (
            StatusCode::SERVICE_UNAVAILABLE,
            Json(HealthResponse {
                status: "degraded",
                service: "quire",
                version: env!("CARGO_PKG_VERSION"),
                error: Some(error.to_string()),
            }),
        )
            .into_response(),
    }
}

#[derive(Serialize)]
struct PublishedResponse {
    published: bool,
    event: &'static str,
    doc_id: DocId,
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
    context: RequestContext,
    Query(query): Query<ListQuery>,
) -> Result<Json<ReportList>, ApiError> {
    let page_size = query.page_size.or(query.limit).unwrap_or(20).clamp(1, 100);
    let offset = query
        .offset
        .or_else(|| query.page.map(|page| (page.max(1) - 1) * page_size))
        .unwrap_or(0)
        .max(0);
    let mut list = state
        .list_reports_authorized(&context, query.query.as_deref(), page_size, offset)
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

#[derive(Debug, Deserialize)]
struct DocumentQuery {
    rev: Option<i64>,
}

async fn get_document(
    State(state): State<AppState>,
    Path(doc_id): Path<DocId>,
    context: RequestContext,
    Query(query): Query<DocumentQuery>,
) -> Result<Json<DocumentView>, ApiError> {
    state.authorize_doc_access(&context, &doc_id, false).await?;
    let document = match query.rev {
        Some(rev) if rev < 1 => {
            return Err(ApiError(StoreError::InvalidRequest(
                "rev must identify a committed revision (1 or higher)".into(),
            )));
        }
        Some(rev) => {
            state
                .store
                .document_at_revision(&doc_id, Revision::new(rev)?)
                .await?
        }
        None => state.store.get_document(&doc_id).await?,
    };
    let computed = compute_document_meta(&document);
    Ok(Json(DocumentView {
        current_rev: document.revision,
        document,
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
    context: RequestContext,
    Query(query): Query<EventsQuery>,
) -> Result<Sse<impl Stream<Item = Result<Event, std::convert::Infallible>>>, ApiError> {
    state.authorize_doc_access(&context, &doc_id, false).await?;
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
    context: RequestContext,
) -> Result<Sse<impl Stream<Item = Result<Event, std::convert::Infallible>>>, ApiError> {
    state
        .authorize_session_access(&context, &session_id)
        .await?;
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
    context: RequestContext,
    Query(query): Query<RevisionsQuery>,
) -> Result<Json<RevisionListView>, ApiError> {
    state.authorize_doc_access(&context, &doc_id, false).await?;
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
    state.authorize_doc_access(&context, &doc_id, true).await?;
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
    context: RequestContext,
    Query(query): Query<DiffQuery>,
) -> Result<Json<DocumentDiffReport>, ApiError> {
    state.authorize_doc_access(&context, &doc_id, false).await?;
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

#[derive(Debug, Deserialize)]
struct SyncQuery {
    #[serde(rename = "fromRev", alias = "from_rev")]
    from_rev: i64,
}

async fn document_sync(
    State(state): State<AppState>,
    Path(doc_id): Path<DocId>,
    context: RequestContext,
    Query(query): Query<SyncQuery>,
) -> Result<Json<DocumentSyncDelta>, ApiError> {
    state.authorize_doc_access(&context, &doc_id, false).await?;
    let from_revision = Revision::new(query.from_rev)?;
    let after = state.store.get_document(&doc_id).await?;
    if from_revision > after.revision {
        return Err(ApiError(StoreError::InvalidRequest(format!(
            "fromRev {} is ahead of current_rev {}",
            from_revision.get(),
            after.revision.get()
        ))));
    }
    let before = if from_revision == after.revision {
        after.clone()
    } else {
        state
            .store
            .document_at_revision(&doc_id, from_revision)
            .await?
    };
    Ok(Json(sync_documents(&before, &after)))
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
    context: RequestContext,
    body: Bytes,
) -> Result<(StatusCode, Json<PublishedResponse>), ApiError> {
    state.authorize_doc_access(&context, &doc_id, true).await?;
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
        Json(PublishedResponse {
            published: true,
            event: payload.event_name(),
            doc_id,
        }),
    ))
}
