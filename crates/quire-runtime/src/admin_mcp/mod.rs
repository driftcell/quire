//! Runtime administration MCP server mounted at `/admin/mcp`.
//!
//! Every tool is a thin receipt wrapper over `KbService`, `ModelRegistry` or `RuntimeService`.
//! The endpoint is protected by the runtime bearer token and is never mounted on the report MCP
//! surface used by authoring sub-agents.

use chrono::{DateTime, Utc};
use quire_session_protocol::{
    ContextUsage, RunTraceSummary, SessionDetail, SessionStatus, SessionSummary, ToolCallDetail,
};
use rmcp::{
    ErrorData, ServerHandler,
    handler::server::{
        router::tool::ToolRouter,
        wrapper::{Json, Parameters},
    },
    model::{CallToolResult, Implementation, ServerCapabilities, ServerConfig},
    tool, tool_handler, tool_router,
};
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use serde_json::Value;

use crate::{
    error::RuntimeError,
    kb::{
        AgentDefinitionRecord, KbBranchDiff, KbBranchRecord, KbBranchStatus, KbDocumentRecord,
        KbItemKind, KbItemSummary, KbPromoteReport, KbRollbackReport, KbSnapshotSummary,
        SkillDefinitionRecord, UpsertKbAgentCommand,
    },
    mcp_registry::{McpServerInput, McpServerRecord, McpSyncReport, McpToolRecord},
    models::{
        ModelInput, ModelRecord, ModelsDevImportRequest, ProviderInput, ProviderProtocol,
        ProviderRecord,
    },
    service::RuntimeService,
    store::{RunRecord, SessionPatch},
};

const SERVER_INSTRUCTIONS: &str = r#"QUIRE RUNTIME ADMIN MCP

This endpoint manages the runtime control plane. It is protected by the runtime bearer token and
must never be exposed to report-authoring sub-agents.

BRANCH WORKFLOW: kb_list_branches -> kb_create_branch -> kb_upsert_doc / kb_upsert_agent /
kb_upsert_skill -> kb_diff_branch -> kb_promote_branch. Promotion performs item-level three-way
merging. Overlapping (item_kind, item_key) edits return structured base/active/branch content;
resolve them inside the draft branch and retry.

ROLLBACK: kb_rollback_active without a version restores the latest earlier snapshot. Pass a version
from kb_list_branches.recent_snapshots for a specific restore point.

MCP REGISTRY: mcp_list_servers -> mcp_upsert_server -> mcp_sync_tools -> mcp_list_tools ->
mcp_set_tool_enabled. Server and tool switches are global and intersect with each agent's
allowed_tools. Auth token references are always masked.

Every receipt includes `_next_hint`."#;

#[derive(Clone)]
pub struct AdminMcpServer {
    service: RuntimeService,
    tool_router: ToolRouter<Self>,
}

impl AdminMcpServer {
    pub fn new(service: RuntimeService) -> Self {
        Self {
            service,
            tool_router: Self::tool_router(),
        }
    }
}

#[tool_handler(router = self.tool_router)]
impl ServerHandler for AdminMcpServer {
    fn get_info(&self) -> ServerConfig {
        ServerConfig::new(ServerCapabilities::builder().enable_tools().build())
            .with_server_info(Implementation::new(
                "quire-runtime-admin",
                env!("CARGO_PKG_VERSION"),
            ))
            .with_instructions(SERVER_INSTRUCTIONS)
    }
}

mod contracts;
mod kb;
mod mcp_registry;
mod models;
mod sessions;

pub use contracts::*;

impl AdminMcpServer {
    pub(crate) fn tool_router() -> ToolRouter<Self> {
        let mut router = Self::kb_router();
        router.merge(Self::models_router());
        router.merge(Self::mcp_registry_router());
        router.merge(Self::sessions_router());
        router
    }
}

#[derive(Serialize)]
struct AdminErrorData<D> {
    code: &'static str,
    hint: &'static str,
    #[serde(skip_serializing_if = "Option::is_none")]
    details: Option<D>,
}

fn serialize_admin_error_data<T: Serialize>(data: T) -> Option<Value> {
    match serde_json::to_value(data) {
        Ok(value) => Some(value),
        Err(error) => {
            tracing::warn!(%error, "failed to serialize admin MCP error data");
            None
        }
    }
}

fn admin_error(error: RuntimeError) -> ErrorData {
    let code = error.code();
    let hint = error.hint();
    match error {
        RuntimeError::KbConflict(report) => {
            let details = serde_json::to_value(report.as_ref()).ok();
            ErrorData::invalid_params(
                "knowledge-base branch promotion conflict",
                serialize_admin_error_data(AdminErrorData {
                    code,
                    hint,
                    details,
                }),
            )
        }
        RuntimeError::SessionNotFound(_)
        | RuntimeError::RunNotFound(_)
        | RuntimeError::ToolCallNotFound(_)
        | RuntimeError::InvalidRequest(_) => ErrorData::invalid_params(
            error.to_string(),
            serialize_admin_error_data(AdminErrorData::<()> {
                code,
                hint,
                details: None,
            }),
        ),
        other => {
            tracing::warn!(error = %other, "admin MCP operation failed");
            ErrorData::internal_error(
                "internal runtime error; retry the request or inspect runtime logs",
                serialize_admin_error_data(AdminErrorData::<()> {
                    code,
                    hint,
                    details: None,
                }),
            )
        }
    }
}
