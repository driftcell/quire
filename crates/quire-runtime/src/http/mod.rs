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
use quire_protocol::{ApiErrorEnvelope, CommentStatus, CommentThread, TextQuoteSelector};
use quire_session_protocol::{
    AttachmentMeta, CreateSessionRequest, PendingQueueItem, QuestionAnswer, RunTraceSummary,
    SendMessageRequest, SessionCommentRef, SessionDetail, SessionEvent, SessionEventKind,
    SessionSummary, ToolCallDetail, UpdatePendingMessageRequest,
};
use rmcp::transport::streamable_http_server::{
    StreamableHttpServerConfig, StreamableHttpService, session::local::LocalSessionManager,
};
use serde::{Deserialize, Serialize};
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
        .route_layer(middleware::from_fn(admin_auth))
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
    let public_auth = Router::new()
        .route("/api/auth/register", post(register))
        .route("/api/auth/login", post(login));
    let user_protected = Router::new()
        .route("/api/auth/me", get(me))
        .route("/api/auth/change-password", post(change_password))
        .route("/api/auth/profile", patch(update_profile))
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
        .route_layer(middleware::from_fn_with_state(
            service.clone(),
            runtime_auth,
        ));
    let admin_protected = Router::new()
        .route("/api/admin/users", get(admin_list_users))
        .route("/api/admin/users/{user_id}", patch(admin_update_user))
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
        .route_layer(middleware::from_fn(admin_auth))
        .route_layer(middleware::from_fn_with_state(
            service.clone(),
            runtime_auth,
        ));
    let document_router = quire_server::build_document_router(service.report_app_state());
    Router::new()
        .route("/health", get(health))
        .merge(public_auth)
        .merge(chart_previews)
        .merge(user_protected)
        .merge(admin_protected)
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

pub(crate) use auth::*;
pub(crate) use auth_routes::*;
pub(crate) use kb::*;
pub(crate) use registry::*;
pub(crate) use session_events::*;
pub(crate) use sessions::*;

mod auth;
mod auth_routes;
mod kb;
mod registry;
mod session_events;
mod sessions;
