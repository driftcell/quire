//! Axum control plane and Session-SSE endpoint.

use std::convert::Infallible;

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
use quire_session_protocol::{
    AttachmentMeta, SendMessageRequest, SessionDetail, SessionEvent, SessionEventKind,
    SessionSummary, ToolCallDetail,
};
use secrecy::ExposeSecret;
use serde::{Deserialize, Serialize};
use serde_json::json;
use subtle::ConstantTimeEq;
use tower_http::cors::{Any, CorsLayer};
use uuid::Uuid;

use crate::{
    error::{RuntimeError, RuntimeResult},
    kb::{AgentDefinitionRecord, KbDocumentRecord, SkillDefinitionRecord},
    models::{
        ModelInput, ModelRecord, ModelsDevImportReport, ModelsDevImportRequest, ProviderInput,
        ProviderRecord,
    },
    service::RuntimeService,
};

pub fn router(service: RuntimeService) -> Router {
    let protected = Router::new()
        .route("/api/sessions", get(list_sessions).post(create_session))
        .route(
            "/api/sessions/{session_id}",
            get(get_session)
                .patch(update_session)
                .delete(delete_session),
        )
        .route("/api/sessions/{session_id}/events", get(session_events))
        .route("/api/sessions/{session_id}/messages", post(post_message))
        .route("/api/sessions/{session_id}/interrupt", post(interrupt))
        .route("/api/sessions/{session_id}/retry", post(retry))
        .route("/api/sessions/{session_id}/compact", post(compact))
        .route(
            "/api/sessions/{session_id}/attachments",
            post(post_session_attachment),
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
        .route_layer(middleware::from_fn_with_state(
            service.clone(),
            runtime_auth,
        ));
    Router::new()
        .route("/health", get(health))
        .merge(protected)
        .layer(
            CorsLayer::new()
                .allow_origin(Any)
                .allow_methods(Any)
                .allow_headers(Any),
        )
        .with_state(service)
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
            Json(json!({
                "error": {
                    "code": "auth_not_configured",
                    "message": "runtime API token is not configured",
                    "hint": "Set RUNTIME_API_TOKEN before exposing quire-runtime."
                }
            })),
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
            Json(json!({
                "error": {
                    "code": "unauthorized",
                    "message": "missing or invalid runtime bearer token",
                    "hint": "Send `Authorization: Bearer $RUNTIME_API_TOKEN`."
                }
            })),
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

async fn health(State(service): State<RuntimeService>) -> Response {
    match service.store().health().await {
        Ok(()) => (
            StatusCode::OK,
            Json(json!({"status":"ok","service":"quire-runtime"})),
        )
            .into_response(),
        Err(error) => (
            StatusCode::SERVICE_UNAVAILABLE,
            Json(json!({"status":"degraded","error":error.to_string()})),
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

#[derive(Debug, Deserialize)]
struct CreateSessionRequest {
    title: String,
    #[serde(default)]
    doc_id: Option<String>,
}

async fn create_session(
    State(service): State<RuntimeService>,
    Json(request): Json<CreateSessionRequest>,
) -> RuntimeResult<Json<SessionSummary>> {
    service
        .create_session(&request.title, request.doc_id.as_deref())
        .await
        .map(Json)
}

async fn get_session(
    State(service): State<RuntimeService>,
    Path(session_id): Path<String>,
) -> RuntimeResult<Json<SessionDetail>> {
    service.session_detail(&session_id).await.map(Json)
}

#[derive(Debug, Deserialize)]
struct UpdateSessionRequest {
    #[serde(default)]
    title: Option<String>,
    #[serde(default)]
    status: Option<quire_session_protocol::SessionStatus>,
}

async fn update_session(
    State(service): State<RuntimeService>,
    Path(session_id): Path<String>,
    Json(request): Json<UpdateSessionRequest>,
) -> RuntimeResult<Json<SessionSummary>> {
    service
        .update_session(&session_id, request.title.as_deref(), request.status)
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
    message: quire_session_protocol::SessionMessage,
    inserted: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    run_id: Option<String>,
}

async fn post_message(
    State(service): State<RuntimeService>,
    Path(session_id): Path<String>,
    Json(request): Json<SendMessageRequest>,
) -> RuntimeResult<Json<PostMessageResponse>> {
    let (message, inserted, run_id) = service.post_message(&session_id, &request).await?;
    Ok(Json(PostMessageResponse {
        message,
        inserted,
        run_id,
    }))
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
    service.compact(&session_id).await.map(Json)
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
        .create_attachment(
            session_id,
            &request.filename,
            &request.mime_type,
            request.size_bytes,
            request.content_base64.as_deref(),
        )
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

async fn session_events(
    State(service): State<RuntimeService>,
    Path(session_id): Path<String>,
    Query(query): Query<EventsQuery>,
) -> Result<Sse<impl Stream<Item = Result<Event, Infallible>>>, RuntimeError> {
    service.store().get_session_summary(&session_id).await?;
    let after_seq = query.after_seq.unwrap_or(0);
    let stream = event_stream(service, session_id, after_seq);
    Ok(Sse::new(stream).keep_alive(KeepAlive::default()))
}

fn event_stream(
    service: RuntimeService,
    session_id: String,
    after_seq: u64,
) -> impl Stream<Item = Result<Event, Infallible>> {
    async_stream::stream! {
        yield Ok(Event::default().comment("connected"));
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
        match store.list_events(&session_id, after_seq, i64::try_from(replay_limit).unwrap_or(500)).await {
            Ok(events) => {
                for event in events {
                    last_seq = event.seq;
                    yield Ok(sse_event(&event));
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
        let mut heartbeat = tokio::time::interval(std::time::Duration::from_secs(15));
        loop {
            tokio::select! {
                event = subscriber.next_event() => {
                    let Some(event) = event else { break };
                    if event.seq <= last_seq {
                        continue;
                    }
                    if event.seq > last_seq + 1 {
                        yield Ok(sse_event(&synthetic_sync(
                            &session_id,
                            last_seq,
                            "live event gap detected",
                        )));
                        break;
                    }
                    last_seq = event.seq;
                    yield Ok(sse_event(&event));
                }
                _ = heartbeat.tick() => {
                    yield Ok(Event::default().comment("heartbeat"));
                }
            }
        }
    }
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
    model: String,
    system_prompt: String,
    #[serde(default)]
    allowed_tools: Vec<String>,
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
        .upsert_agent(
            &name,
            &request.model,
            &request.system_prompt,
            &request.allowed_tools,
            request.enabled,
        )
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
        .upsert_agent(
            &name,
            &request.model,
            &request.system_prompt,
            &request.allowed_tools,
            request.enabled,
        )
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

/// Reserved for attachment IDs generated by clients before upload.
pub fn new_attachment_id() -> String {
    format!("att_{}", Uuid::new_v4().simple())
}
