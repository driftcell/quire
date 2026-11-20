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
        SkillDefinitionRecord,
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

#[derive(Debug, Default, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct EmptyParams {}

#[derive(Debug, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct KbCreateBranchParams {
    /// New branch name: 1-64 letters, digits, '-', '_' or '.', starting alphanumeric.
    #[serde(alias = "branch")]
    pub name: String,
    /// Human-readable purpose of the branch. May be empty.
    #[serde(default)]
    pub description: String,
    /// Source branch. Defaults to the current active branch.
    #[serde(default, alias = "base", alias = "source", alias = "from_branch")]
    pub from: Option<String>,
}

#[derive(Debug, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct KbBranchParams {
    /// Branch name returned by kb_list_branches.
    #[serde(alias = "branch")]
    pub name: String,
}

#[derive(Debug, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct KbRollbackParams {
    /// Snapshot version to restore. Omit to restore the latest earlier snapshot.
    #[serde(default, alias = "snapshot_version", alias = "to_version")]
    pub version: Option<u64>,
}

#[derive(Debug, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct KbListItemsParams {
    /// Branch to inspect.
    pub branch: String,
    /// Optional item kind filter.
    #[serde(default)]
    pub item_kind: Option<KbItemKind>,
}

#[derive(Debug, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct KbReadItemParams {
    /// Branch to inspect.
    pub branch: String,
    /// doc, agent or skill.
    #[serde(alias = "kind")]
    pub item_kind: KbItemKind,
    /// doc_id for documents, name for agents and skills.
    #[serde(alias = "key")]
    pub item_key: String,
}

#[derive(Debug, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct KbUpsertDocParams {
    /// Branch to edit. Use the active branch only for deliberate production changes.
    pub branch: String,
    /// Stable document identifier.
    #[serde(alias = "id")]
    pub doc_id: String,
    /// Display name.
    pub name: String,
    /// Full markdown content.
    pub content: String,
}

#[derive(Debug, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct KbUpsertAgentParams {
    /// Branch to edit.
    pub branch: String,
    /// Agent name, for example `main` or `reviewer`.
    pub name: String,
    /// Short description of when to delegate to this agent.
    #[serde(default)]
    pub description: String,
    /// Registered model route ID.
    pub model: String,
    /// Full system prompt.
    pub system_prompt: String,
    /// MCP tool names the agent may call.
    #[serde(default, alias = "tools")]
    pub allowed_tools: Vec<String>,
    /// Whether this agent is the default primary agent for the branch.
    #[serde(default)]
    pub is_default: bool,
    /// Whether the agent is enabled. Default true.
    #[serde(default = "default_enabled")]
    pub enabled: bool,
}

#[derive(Debug, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct SkillResourceParam {
    /// Path relative to the skill directory.
    pub path: String,
    /// Full resource content.
    pub content: String,
}

#[derive(Debug, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct KbUpsertSkillParams {
    /// Branch to edit.
    pub branch: String,
    /// Skill name.
    pub name: String,
    /// Short description shown in the system prompt.
    pub description: String,
    /// Skill body markdown.
    pub body: String,
    /// Whether the skill is enabled. Default true.
    #[serde(default = "default_enabled")]
    pub enabled: bool,
    /// Optional skill resources.
    #[serde(default)]
    pub resources: Vec<SkillResourceParam>,
}

#[derive(Debug, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct ModelsUpsertProviderParams {
    /// Stable provider ID.
    pub provider_id: String,
    /// Display name.
    pub name: String,
    /// Wire protocol.
    pub protocol: ProviderProtocol,
    /// API base URL.
    pub base_url: String,
    /// `env:VAR`, `enc:...` or a raw key when RUNTIME_MASTER_KEY is configured.
    pub api_key_ref: String,
    /// Whether the provider is enabled. Default true.
    #[serde(default = "default_enabled")]
    pub enabled: bool,
}

#[derive(Debug, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct ModelsImportParams {
    /// Local provider receiving the imported models.
    pub provider_id: String,
    /// Optional upstream models.dev provider ID filter.
    #[serde(default)]
    pub provider_filter: Option<String>,
    /// Optional subset of upstream model IDs.
    #[serde(default)]
    pub model_ids: Option<Vec<String>>,
    /// Inline models.dev payload for offline imports.
    #[serde(default)]
    pub payload: Option<Value>,
    /// Optional catalog URL override.
    #[serde(default)]
    pub url: Option<String>,
}

#[derive(Debug, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct SessionsListParams {
    /// Page size, 1..=200. Default 50.
    #[serde(default)]
    pub limit: Option<i64>,
    /// Offset. Default 0.
    #[serde(default)]
    pub offset: Option<i64>,
}

#[derive(Debug, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct SessionsInspectParams {
    /// Session ID to inspect.
    pub session_id: String,
}

#[derive(Debug, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct KbDeleteItemParams {
    /// Target branch. Omit to use the active branch.
    #[serde(default)]
    pub branch: Option<String>,
    /// doc, agent or skill.
    #[serde(alias = "kind")]
    pub item_kind: KbItemKind,
    /// doc_id for documents, name for agents and skills.
    #[serde(alias = "key", alias = "id")]
    pub item_key: String,
}

#[derive(Debug, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct ModelIdParams {
    /// Registered model route ID.
    pub id: String,
}

#[derive(Debug, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct ProviderIdParams {
    /// Registered provider ID.
    pub provider_id: String,
}

#[derive(Debug, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct SessionsToolCallParams {
    /// Session ID.
    pub session_id: String,
    /// Tool call ID from run traces or stream events.
    pub tool_call_id: String,
}

#[derive(Debug, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct SessionsUpdateParams {
    /// Session ID to update.
    pub session_id: String,
    /// Optional new title.
    #[serde(default)]
    pub title: Option<String>,
    /// Optional new status (active or archived).
    #[serde(default)]
    pub status: Option<SessionStatus>,
    /// Optional pin state. `true` pins with the current timestamp; `false`
    /// unpins. Omit to keep the current value.
    #[serde(default)]
    pub pinned: Option<bool>,
}

#[derive(Debug, Clone, Copy, Deserialize, Serialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum SessionAdminAction {
    Interrupt,
    Retry,
    Compact,
    Delete,
}

#[derive(Debug, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct SessionsActionParams {
    /// Target session ID.
    pub session_id: String,
    /// Action to perform: interrupt, retry, compact or delete.
    pub action: SessionAdminAction,
    /// Optional run ID when action is `interrupt`.
    #[serde(default)]
    pub run_id: Option<String>,
}

#[derive(Debug, Serialize, JsonSchema)]
pub struct DeleteReceipt {
    pub deleted: bool,
    pub target: String,
    #[serde(rename = "_next_hint")]
    pub next_hint: String,
}

#[derive(Debug, Serialize, JsonSchema)]
pub struct ModelReceipt {
    pub model: ModelRecord,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub warnings: Vec<String>,
    #[serde(rename = "_next_hint")]
    pub next_hint: String,
}

#[derive(Debug, Serialize, JsonSchema)]
pub struct SessionsTracesReceipt {
    pub session_id: String,
    pub traces: Vec<RunTraceSummary>,
    #[serde(rename = "_next_hint")]
    pub next_hint: String,
}

#[derive(Debug, Serialize, JsonSchema)]
pub struct SessionsToolCallReceipt {
    pub tool_call: ToolCallDetail,
    #[serde(rename = "_next_hint")]
    pub next_hint: String,
}

#[derive(Debug, Serialize, JsonSchema)]
pub struct SessionsUpdateReceipt {
    pub session: SessionSummary,
    #[serde(rename = "_next_hint")]
    pub next_hint: String,
}

#[derive(Debug, Serialize, JsonSchema)]
pub struct SessionsActionReceipt {
    pub session_id: String,
    pub action: SessionAdminAction,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub session: Option<SessionDetail>,
    #[serde(rename = "_next_hint")]
    pub next_hint: String,
}

#[derive(Debug, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct McpSyncParams {
    /// Registered MCP server ID.
    pub server_id: String,
}

#[derive(Debug, Default, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct McpListToolsParams {
    /// Optional server filter.
    #[serde(default)]
    pub server_id: Option<String>,
}

#[derive(Debug, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct McpSetToolEnabledParams {
    /// Registered MCP server ID.
    pub server_id: String,
    /// Tool name returned by mcp_list_tools.
    pub tool_name: String,
    /// Global enable switch.
    pub enabled: bool,
}

#[derive(Debug, Serialize, JsonSchema)]
pub struct KbListBranchesReceipt {
    /// Current production branch.
    pub active_branch: String,
    pub branches: Vec<KbBranchRecord>,
    /// Most recent immutable snapshots available to kb_rollback_active.
    pub recent_snapshots: Vec<KbSnapshotSummary>,
    #[serde(rename = "_next_hint")]
    pub next_hint: String,
}

#[derive(Debug, Serialize, JsonSchema)]
pub struct KbCreateBranchReceipt {
    pub branch: KbBranchRecord,
    #[serde(rename = "_next_hint")]
    pub next_hint: String,
}

#[derive(Debug, Serialize, JsonSchema)]
pub struct KbDiffBranchReceipt {
    pub diff: KbBranchDiff,
    #[serde(rename = "_next_hint")]
    pub next_hint: String,
}

#[derive(Debug, Serialize, JsonSchema)]
pub struct KbPromoteBranchReceipt {
    pub report: KbPromoteReport,
    #[serde(rename = "_next_hint")]
    pub next_hint: String,
}

#[derive(Debug, Serialize, JsonSchema)]
pub struct KbRollbackReceipt {
    pub report: KbRollbackReport,
    #[serde(rename = "_next_hint")]
    pub next_hint: String,
}

#[derive(Debug, Serialize, JsonSchema)]
pub struct KbListItemsReceipt {
    pub branch: String,
    pub items: Vec<KbItemSummary>,
    #[serde(rename = "_next_hint")]
    pub next_hint: String,
}

#[derive(Debug, Serialize, JsonSchema)]
pub struct KbReadItemReceipt {
    pub branch: String,
    pub item_kind: KbItemKind,
    pub item_key: String,
    pub found: bool,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub content: Option<Value>,
    #[serde(rename = "_next_hint")]
    pub next_hint: String,
}

#[derive(Debug, Serialize, JsonSchema)]
pub struct KbDocumentReceipt {
    pub document: KbDocumentRecord,
    #[serde(rename = "_next_hint")]
    pub next_hint: String,
}

#[derive(Debug, Serialize, JsonSchema)]
pub struct KbAgentReceipt {
    pub agent: AgentDefinitionRecord,
    #[serde(rename = "_next_hint")]
    pub next_hint: String,
}

#[derive(Debug, Serialize, JsonSchema)]
pub struct KbSkillReceipt {
    pub skill: SkillDefinitionRecord,
    #[serde(rename = "_next_hint")]
    pub next_hint: String,
}

#[derive(Debug, Serialize, JsonSchema)]
pub struct ModelsProvidersReceipt {
    /// Provider API keys are always masked.
    pub providers: Vec<ProviderRecord>,
    #[serde(rename = "_next_hint")]
    pub next_hint: String,
}

#[derive(Debug, Serialize, JsonSchema)]
pub struct ModelProviderReceipt {
    pub provider: ProviderRecord,
    #[serde(rename = "_next_hint")]
    pub next_hint: String,
}

#[derive(Debug, Serialize, JsonSchema)]
pub struct ModelsListReceipt {
    pub models: Vec<ModelRecord>,
    #[serde(rename = "_next_hint")]
    pub next_hint: String,
}

#[derive(Debug, Serialize, JsonSchema)]
pub struct ModelsImportReceipt {
    pub provider_id: String,
    pub imported: usize,
    pub models: Vec<ModelRecord>,
    #[serde(rename = "_next_hint")]
    pub next_hint: String,
}

#[derive(Debug, Serialize, JsonSchema)]
pub struct SessionsListReceipt {
    pub sessions: Vec<SessionSummary>,
    #[serde(rename = "_next_hint")]
    pub next_hint: String,
}

#[derive(Debug, Clone, Serialize, JsonSchema)]
pub struct RunSummaryView {
    pub run_id: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub parent_run_id: Option<String>,
    pub agent_id: String,
    pub status: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub model: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub error: Option<String>,
    pub started_at: DateTime<Utc>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub finished_at: Option<DateTime<Utc>>,
}

impl RunSummaryView {
    fn from_record(record: RunRecord) -> Self {
        Self {
            run_id: record.run_id,
            parent_run_id: record.parent_run_id,
            agent_id: record.agent_id,
            status: record.status,
            model: record.model,
            error: record.error,
            started_at: record.started_at,
            finished_at: record.finished_at,
        }
    }
}

#[derive(Debug, Serialize, JsonSchema)]
pub struct SessionsInspectReceipt {
    pub session: SessionDetail,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub latest_run: Option<RunSummaryView>,
    pub context_usage: ContextUsage,
    #[serde(rename = "_next_hint")]
    pub next_hint: String,
}

#[derive(Debug, Serialize, JsonSchema)]
pub struct McpServersReceipt {
    /// Auth token references are always masked.
    pub servers: Vec<McpServerRecord>,
    #[serde(rename = "_next_hint")]
    pub next_hint: String,
}

#[derive(Debug, Serialize, JsonSchema)]
pub struct McpServerReceipt {
    pub server: McpServerRecord,
    #[serde(rename = "_next_hint")]
    pub next_hint: String,
}

#[derive(Debug, Serialize, JsonSchema)]
pub struct McpSyncReceipt {
    pub report: McpSyncReport,
    #[serde(rename = "_next_hint")]
    pub next_hint: String,
}

#[derive(Debug, Serialize, JsonSchema)]
pub struct McpToolsReceipt {
    pub tools: Vec<McpToolRecord>,
    #[serde(rename = "_next_hint")]
    pub next_hint: String,
}

#[derive(Debug, Serialize, JsonSchema)]
pub struct McpToolReceipt {
    pub tool: McpToolRecord,
    #[serde(rename = "_next_hint")]
    pub next_hint: String,
}

fn default_enabled() -> bool {
    true
}

#[tool_router(router = tool_router, vis = "pub(crate)")]
impl AdminMcpServer {
    // ---------------------------------------------------------------------
    // Branch and release tools
    // ---------------------------------------------------------------------

    #[tool(
        name = "preview_report",
        description = "Render a Quire report preview via controlled Playwright MCP, returning an ImageContent screenshot plus structured layout diagnostics and the preview URL. Set include_image=false to skip the PNG."
    )]
    pub async fn preview_report(
        &self,
        Parameters(request): Parameters<crate::preview::PreviewReportInput>,
    ) -> Result<CallToolResult, ErrorData> {
        let result = crate::preview::execute_preview_report(self.service.config(), request)
            .await
            .map_err(admin_error)?;
        Ok(result.to_mcp_call_tool_result())
    }

    #[tool(
        name = "kb_list_branches",
        description = "List knowledge-base branches with status, base/head versions and the latest immutable snapshots."
    )]
    pub async fn kb_list_branches(
        &self,
        Parameters(_request): Parameters<EmptyParams>,
    ) -> Result<Json<KbListBranchesReceipt>, ErrorData> {
        let branches = self
            .service
            .kb()
            .list_branches()
            .await
            .map_err(admin_error)?;
        let active_branch = self
            .service
            .kb()
            .active_branch()
            .await
            .map_err(admin_error)?
            .name;
        let recent_snapshots = self
            .service
            .kb()
            .list_snapshots(20)
            .await
            .map_err(admin_error)?;
        let next_hint = if branches
            .iter()
            .any(|branch| branch.status == KbBranchStatus::Draft)
        {
            "Call kb_diff_branch, edit the draft with kb_upsert_*, then kb_promote_branch."
                .to_string()
        } else {
            "Call kb_create_branch to start an isolated draft.".to_string()
        };
        Ok(Json(KbListBranchesReceipt {
            active_branch,
            branches,
            recent_snapshots,
            next_hint,
        }))
    }

    #[tool(
        name = "kb_create_branch",
        description = "Create an isolated draft branch by copying the active branch at its current version."
    )]
    pub async fn kb_create_branch(
        &self,
        Parameters(request): Parameters<KbCreateBranchParams>,
    ) -> Result<Json<KbCreateBranchReceipt>, ErrorData> {
        let branch = self
            .service
            .kb()
            .create_branch(&request.name, &request.description, request.from.as_deref())
            .await
            .map_err(admin_error)?;
        let next_hint = format!(
            "Edit branch `{}` with kb_upsert_* and inspect it with kb_diff_branch before promotion.",
            branch.name
        );
        Ok(Json(KbCreateBranchReceipt { branch, next_hint }))
    }

    #[tool(
        name = "kb_diff_branch",
        description = "Three-way diff of a draft branch against the active branch and its base snapshot, including conflicts."
    )]
    pub async fn kb_diff_branch(
        &self,
        Parameters(request): Parameters<KbBranchParams>,
    ) -> Result<Json<KbDiffBranchReceipt>, ErrorData> {
        let diff = self
            .service
            .kb()
            .diff_branch(&request.name)
            .await
            .map_err(admin_error)?;
        let next_hint = if diff.changes.iter().any(|change| change.conflict) {
            "Resolve conflict items with kb_upsert_* or delete them, then run kb_diff_branch again."
                .to_string()
        } else {
            "Call kb_promote_branch to fast-forward or auto-merge into the active branch."
                .to_string()
        };
        Ok(Json(KbDiffBranchReceipt { diff, next_hint }))
    }

    #[tool(
        name = "kb_promote_branch",
        description = "Promote a draft branch with Smart CAS: fast-forward, disjoint auto-merge, or a structured conflict error."
    )]
    pub async fn kb_promote_branch(
        &self,
        Parameters(request): Parameters<KbBranchParams>,
    ) -> Result<Json<KbPromoteBranchReceipt>, ErrorData> {
        let report = self
            .service
            .kb()
            .promote_branch(&request.name)
            .await
            .map_err(admin_error)?;
        let next_hint = format!(
            "Active version {} is live and snapshot {} is immutable. Use kb_rollback_active to restore an earlier snapshot.",
            report.version, report.snapshot_version
        );
        Ok(Json(KbPromoteBranchReceipt { report, next_hint }))
    }

    #[tool(
        name = "kb_rollback_active",
        description = "Roll the active branch back to an immutable snapshot. Omit version for the latest earlier snapshot."
    )]
    pub async fn kb_rollback_active(
        &self,
        Parameters(request): Parameters<KbRollbackParams>,
    ) -> Result<Json<KbRollbackReceipt>, ErrorData> {
        let report = self
            .service
            .kb()
            .rollback_active(request.version)
            .await
            .map_err(admin_error)?;
        let next_hint = format!(
            "Restored snapshot {} as active version {}. Cache invalidation was published.",
            report.restored_from_version, report.version
        );
        Ok(Json(KbRollbackReceipt { report, next_hint }))
    }

    // ---------------------------------------------------------------------
    // Branch item editing
    // ---------------------------------------------------------------------

    #[tool(
        name = "kb_list_items",
        description = "List documents, agents and skills in a branch, optionally filtered by item kind."
    )]
    pub async fn kb_list_items(
        &self,
        Parameters(request): Parameters<KbListItemsParams>,
    ) -> Result<Json<KbListItemsReceipt>, ErrorData> {
        let mut items = self
            .service
            .kb()
            .list_items(&request.branch)
            .await
            .map_err(admin_error)?;
        if let Some(kind) = request.item_kind {
            items.retain(|item| item.item_kind == kind);
        }
        let next_hint = "Call kb_read_item with the item key for full content.".to_string();
        Ok(Json(KbListItemsReceipt {
            branch: request.branch,
            items,
            next_hint,
        }))
    }

    #[tool(
        name = "kb_read_item",
        description = "Read the full content of one document, agent or skill in a branch."
    )]
    pub async fn kb_read_item(
        &self,
        Parameters(request): Parameters<KbReadItemParams>,
    ) -> Result<Json<KbReadItemReceipt>, ErrorData> {
        let content = self
            .service
            .kb()
            .read_item(&request.branch, request.item_kind, &request.item_key)
            .await
            .map_err(admin_error)?;
        let found = content.is_some();
        let next_hint = if found {
            "Edit with kb_upsert_doc, kb_upsert_agent or kb_upsert_skill, then inspect kb_diff_branch."
                .to_string()
        } else {
            "Item not found. Call kb_list_items to discover valid keys.".to_string()
        };
        Ok(Json(KbReadItemReceipt {
            branch: request.branch,
            item_kind: request.item_kind,
            item_key: request.item_key,
            found,
            content,
            next_hint,
        }))
    }

    #[tool(
        name = "kb_upsert_doc",
        description = "Create or update a knowledge-base document inside a branch."
    )]
    pub async fn kb_upsert_doc(
        &self,
        Parameters(request): Parameters<KbUpsertDocParams>,
    ) -> Result<Json<KbDocumentReceipt>, ErrorData> {
        let document = self
            .service
            .kb()
            .upsert_document_on(
                &request.branch,
                &request.doc_id,
                &request.name,
                &request.content,
            )
            .await
            .map_err(admin_error)?;
        let next_hint = format!(
            "Document `{}` updated in branch `{}`. Run kb_diff_branch before promotion.",
            document.doc_id, request.branch
        );
        Ok(Json(KbDocumentReceipt {
            document,
            next_hint,
        }))
    }

    #[tool(
        name = "kb_upsert_agent",
        description = "Create or update an agent definition inside a branch."
    )]
    pub async fn kb_upsert_agent(
        &self,
        Parameters(request): Parameters<KbUpsertAgentParams>,
    ) -> Result<Json<KbAgentReceipt>, ErrorData> {
        let agent = self
            .service
            .kb()
            .upsert_agent_on(
                &request.branch,
                &request.name,
                &request.description,
                &request.model,
                &request.system_prompt,
                &request.allowed_tools,
                request.is_default,
                request.enabled,
            )
            .await
            .map_err(admin_error)?;
        let next_hint = format!(
            "Agent `{}` updated in branch `{}`. Test it with a session created on that branch.",
            agent.name, request.branch
        );
        Ok(Json(KbAgentReceipt { agent, next_hint }))
    }

    #[tool(
        name = "kb_upsert_skill",
        description = "Create or update a skill definition and its resources inside a branch."
    )]
    pub async fn kb_upsert_skill(
        &self,
        Parameters(request): Parameters<KbUpsertSkillParams>,
    ) -> Result<Json<KbSkillReceipt>, ErrorData> {
        let resources: Vec<(String, String)> = request
            .resources
            .into_iter()
            .map(|resource| (resource.path, resource.content))
            .collect();
        let skill = self
            .service
            .kb()
            .upsert_skill_on(
                &request.branch,
                &request.name,
                &request.description,
                &request.body,
                request.enabled,
                &resources,
            )
            .await
            .map_err(admin_error)?;
        let next_hint = format!(
            "Skill `{}` updated in branch `{}`. Call kb_diff_branch to review the release.",
            skill.name, request.branch
        );
        Ok(Json(KbSkillReceipt { skill, next_hint }))
    }

    #[tool(
        name = "kb_delete_item",
        description = "Delete a document, agent or skill from a KB branch (defaults to active branch)."
    )]
    pub async fn kb_delete_item(
        &self,
        Parameters(request): Parameters<KbDeleteItemParams>,
    ) -> Result<Json<DeleteReceipt>, ErrorData> {
        let branch = match request.branch {
            Some(branch) => branch,
            None => {
                self.service
                    .kb()
                    .active_branch()
                    .await
                    .map_err(admin_error)?
                    .name
            }
        };
        match request.item_kind {
            KbItemKind::Doc => self
                .service
                .kb()
                .delete_document_on(&branch, &request.item_key)
                .await
                .map_err(admin_error)?,
            KbItemKind::Agent => self
                .service
                .kb()
                .delete_agent_on(&branch, &request.item_key)
                .await
                .map_err(admin_error)?,
            KbItemKind::Skill => self
                .service
                .kb()
                .delete_skill_on(&branch, &request.item_key)
                .await
                .map_err(admin_error)?,
        }
        let target = format!("{branch}:{:?}:{}", request.item_kind, request.item_key);
        let next_hint = format!(
            "Deleted `{}` from branch `{}`. Call kb_list_items to verify.",
            request.item_key, branch
        );
        Ok(Json(DeleteReceipt {
            deleted: true,
            target,
            next_hint,
        }))
    }

    // ---------------------------------------------------------------------
    // Model and provider registry
    // ---------------------------------------------------------------------

    #[tool(
        name = "models_list_providers",
        description = "List LLM providers. API key references are always masked."
    )]
    pub async fn models_list_providers(
        &self,
        Parameters(_request): Parameters<EmptyParams>,
    ) -> Result<Json<ModelsProvidersReceipt>, ErrorData> {
        let providers = self
            .service
            .models()
            .list_providers()
            .await
            .map_err(admin_error)?;
        let next_hint =
            "Call models_list to inspect routes or models_upsert_provider to add one.".to_string();
        Ok(Json(ModelsProvidersReceipt {
            providers,
            next_hint,
        }))
    }

    #[tool(
        name = "models_upsert_provider",
        description = "Create or update an LLM provider. Raw API keys are encrypted and never returned."
    )]
    pub async fn models_upsert_provider(
        &self,
        Parameters(request): Parameters<ModelsUpsertProviderParams>,
    ) -> Result<Json<ModelProviderReceipt>, ErrorData> {
        let provider = self
            .service
            .models()
            .upsert_provider(ProviderInput {
                provider_id: request.provider_id,
                name: request.name,
                protocol: request.protocol,
                base_url: request.base_url,
                api_key_ref: request.api_key_ref,
                enabled: request.enabled,
            })
            .await
            .map_err(admin_error)?;
        let next_hint =
            "Import routes with models_import_models_dev, then bind them to agents with kb_upsert_agent."
                .to_string();
        Ok(Json(ModelProviderReceipt {
            provider,
            next_hint,
        }))
    }

    #[tool(
        name = "models_list",
        description = "List registered model routes with limits, capabilities and pricing."
    )]
    pub async fn models_list(
        &self,
        Parameters(_request): Parameters<EmptyParams>,
    ) -> Result<Json<ModelsListReceipt>, ErrorData> {
        let models = self
            .service
            .models()
            .list_models()
            .await
            .map_err(admin_error)?;
        let next_hint =
            "Use a model route ID when creating an agent with kb_upsert_agent.".to_string();
        Ok(Json(ModelsListReceipt { models, next_hint }))
    }

    #[tool(
        name = "models_import_models_dev",
        description = "Import models.dev catalog entries into an existing provider, using an inline payload or URL."
    )]
    pub async fn models_import_models_dev(
        &self,
        Parameters(request): Parameters<ModelsImportParams>,
    ) -> Result<Json<ModelsImportReceipt>, ErrorData> {
        let report = self
            .service
            .models()
            .import_models_dev(ModelsDevImportRequest {
                provider_id: request.provider_id,
                provider_filter: request.provider_filter,
                model_ids: request.model_ids,
                payload: request.payload,
                url: request.url,
            })
            .await
            .map_err(admin_error)?;
        let next_hint = if report.imported == 0 {
            "No models matched the filter. Call models_import_models_dev without provider_filter or model_ids."
                .to_string()
        } else {
            format!(
                "Imported {} routes for provider `{}`. Call models_list to inspect them.",
                report.imported, report.provider_id
            )
        };
        Ok(Json(ModelsImportReceipt {
            provider_id: report.provider_id,
            imported: report.imported,
            models: report.models,
            next_hint,
        }))
    }

    #[tool(
        name = "models_upsert",
        description = "Manually create or update a single model route configuration."
    )]
    pub async fn models_upsert(
        &self,
        Parameters(request): Parameters<ModelInput>,
    ) -> Result<Json<ModelReceipt>, ErrorData> {
        let model = self
            .service
            .models()
            .upsert_model(request)
            .await
            .map_err(admin_error)?;
        let warnings = self
            .service
            .models()
            .preflight_warnings(&model.id)
            .await
            .unwrap_or_else(|error| vec![format!("preflight unavailable: {error}")]);
        let next_hint = format!(
            "Model `{}` saved under provider `{}`. Reference it in kb_upsert_agent.",
            model.id, model.provider_id
        );
        Ok(Json(ModelReceipt {
            model,
            warnings,
            next_hint,
        }))
    }

    #[tool(
        name = "models_delete",
        description = "Delete a registered model route by ID."
    )]
    pub async fn models_delete(
        &self,
        Parameters(request): Parameters<ModelIdParams>,
    ) -> Result<Json<DeleteReceipt>, ErrorData> {
        self.service
            .models()
            .delete_model(&request.id)
            .await
            .map_err(admin_error)?;
        let next_hint = format!(
            "Model `{}` deleted. Call models_list to verify remaining routes.",
            request.id
        );
        Ok(Json(DeleteReceipt {
            deleted: true,
            target: request.id,
            next_hint,
        }))
    }

    #[tool(
        name = "models_delete_provider",
        description = "Delete a model provider and its associated model routes."
    )]
    pub async fn models_delete_provider(
        &self,
        Parameters(request): Parameters<ProviderIdParams>,
    ) -> Result<Json<DeleteReceipt>, ErrorData> {
        self.service
            .models()
            .delete_provider(&request.provider_id)
            .await
            .map_err(admin_error)?;
        let next_hint = format!(
            "Provider `{}` deleted. Call models_list_providers to verify.",
            request.provider_id
        );
        Ok(Json(DeleteReceipt {
            deleted: true,
            target: request.provider_id,
            next_hint,
        }))
    }

    // ---------------------------------------------------------------------
    // Global MCP registry
    // ---------------------------------------------------------------------

    #[tool(
        name = "mcp_list_servers",
        description = "List registered MCP servers. Auth token references are always masked."
    )]
    pub async fn mcp_list_servers(
        &self,
        Parameters(_request): Parameters<EmptyParams>,
    ) -> Result<Json<McpServersReceipt>, ErrorData> {
        let servers = self
            .service
            .mcp()
            .list_servers()
            .await
            .map_err(admin_error)?;
        let next_hint =
            "Call mcp_sync_tools after registering a server, then inspect mcp_list_tools."
                .to_string();
        Ok(Json(McpServersReceipt { servers, next_hint }))
    }

    #[tool(
        name = "mcp_upsert_server",
        description = "Create or update an MCP server. Raw tokens are encrypted and never returned."
    )]
    pub async fn mcp_upsert_server(
        &self,
        Parameters(request): Parameters<McpServerInput>,
    ) -> Result<Json<McpServerReceipt>, ErrorData> {
        let server = self
            .service
            .mcp()
            .upsert_server(request)
            .await
            .map_err(admin_error)?;
        let next_hint = format!(
            "Server `{}` saved. Run mcp_sync_tools to discover its tool list.",
            server.server_id
        );
        Ok(Json(McpServerReceipt { server, next_hint }))
    }

    #[tool(
        name = "mcp_sync_tools",
        description = "Discover a server's tools and sync descriptions/schemas while preserving admin enable switches."
    )]
    pub async fn mcp_sync_tools(
        &self,
        Parameters(request): Parameters<McpSyncParams>,
    ) -> Result<Json<McpSyncReceipt>, ErrorData> {
        let report = self
            .service
            .mcp()
            .sync_server_tools(&request.server_id)
            .await
            .map_err(admin_error)?;
        let next_hint = format!(
            "Discovered {} tools ({} new, {} updated). Use mcp_set_tool_enabled to disable unsafe tools.",
            report.discovered, report.created, report.updated
        );
        Ok(Json(McpSyncReceipt { report, next_hint }))
    }

    #[tool(
        name = "mcp_list_tools",
        description = "List synced MCP tools and their global enable switch."
    )]
    pub async fn mcp_list_tools(
        &self,
        Parameters(request): Parameters<McpListToolsParams>,
    ) -> Result<Json<McpToolsReceipt>, ErrorData> {
        let tools = self
            .service
            .mcp()
            .list_tools(request.server_id.as_deref())
            .await
            .map_err(admin_error)?;
        let next_hint =
            "Call mcp_set_tool_enabled with the server_id/tool_name to change the global switch."
                .to_string();
        Ok(Json(McpToolsReceipt { tools, next_hint }))
    }

    #[tool(
        name = "mcp_set_tool_enabled",
        description = "Globally enable or disable one MCP tool. Disabled tools are filtered even when agent-allowed."
    )]
    pub async fn mcp_set_tool_enabled(
        &self,
        Parameters(request): Parameters<McpSetToolEnabledParams>,
    ) -> Result<Json<McpToolReceipt>, ErrorData> {
        let tool = self
            .service
            .mcp()
            .set_tool_enabled(&request.server_id, &request.tool_name, request.enabled)
            .await
            .map_err(admin_error)?;
        let next_hint = format!(
            "Tool `{}:{}` is now {}. The next agent run sees the updated global switch.",
            tool.server_id,
            tool.tool_name,
            if tool.enabled { "enabled" } else { "disabled" }
        );
        Ok(Json(McpToolReceipt { tool, next_hint }))
    }

    #[tool(
        name = "mcp_delete_server",
        description = "Delete a registered MCP server and its synced tool records."
    )]
    pub async fn mcp_delete_server(
        &self,
        Parameters(request): Parameters<McpSyncParams>,
    ) -> Result<Json<DeleteReceipt>, ErrorData> {
        self.service
            .mcp()
            .delete_server(&request.server_id)
            .await
            .map_err(admin_error)?;
        let next_hint = format!(
            "Server `{}` deleted. Call mcp_list_servers to inspect remaining servers.",
            request.server_id
        );
        Ok(Json(DeleteReceipt {
            deleted: true,
            target: request.server_id,
            next_hint,
        }))
    }

    // ---------------------------------------------------------------------
    // Session observation
    // ---------------------------------------------------------------------

    #[tool(
        name = "sessions_list",
        description = "List agent sessions newest first, including status, active run status and KB branch."
    )]
    pub async fn sessions_list(
        &self,
        Parameters(request): Parameters<SessionsListParams>,
    ) -> Result<Json<SessionsListReceipt>, ErrorData> {
        let sessions = self
            .service
            .list_sessions(request.limit.unwrap_or(50), request.offset.unwrap_or(0))
            .await
            .map_err(admin_error)?;
        let next_hint = if sessions.is_empty() {
            "No sessions yet. Create one through POST /api/sessions with an optional kb_branch."
                .to_string()
        } else {
            "Call sessions_inspect with a session_id for messages, latest run and context usage."
                .to_string()
        };
        Ok(Json(SessionsListReceipt {
            sessions,
            next_hint,
        }))
    }

    #[tool(
        name = "sessions_inspect",
        description = "Inspect a session: full detail, latest run status and context usage."
    )]
    pub async fn sessions_inspect(
        &self,
        Parameters(request): Parameters<SessionsInspectParams>,
    ) -> Result<Json<SessionsInspectReceipt>, ErrorData> {
        let inspection = self
            .service
            .inspect_session(&request.session_id)
            .await
            .map_err(admin_error)?;
        let context_usage = inspection.detail.summary.context_usage.clone();
        let latest_run = inspection.latest_run.map(RunSummaryView::from_record);
        let next_hint = match latest_run.as_ref().map(|run| run.status.as_str()) {
            Some("running") => "Run is active. Watch Session-SSE or call sessions_inspect again."
                .to_string(),
            Some("failed") => {
                "Latest run failed. Read the run error, fix the branch/model configuration, then retry."
                    .to_string()
            }
            Some(_) => "Run completed. Compare branch content with kb_diff_branch before promotion."
                .to_string(),
            None => "Session has no runs yet. POST a message to start the first turn.".to_string(),
        };
        Ok(Json(SessionsInspectReceipt {
            session: inspection.detail,
            latest_run,
            context_usage,
            next_hint,
        }))
    }

    #[tool(
        name = "sessions_traces",
        description = "List run trace summaries (thinking, tool calls, subagents, errors) for a session."
    )]
    pub async fn sessions_traces(
        &self,
        Parameters(request): Parameters<SessionsInspectParams>,
    ) -> Result<Json<SessionsTracesReceipt>, ErrorData> {
        let traces = self
            .service
            .list_run_traces(&request.session_id)
            .await
            .map_err(admin_error)?;
        let next_hint = if traces.is_empty() {
            "No run traces recorded for this session yet.".to_string()
        } else {
            "Call sessions_get_tool_call with a tool_call_id to inspect full input/output."
                .to_string()
        };
        Ok(Json(SessionsTracesReceipt {
            session_id: request.session_id,
            traces,
            next_hint,
        }))
    }

    #[tool(
        name = "sessions_get_tool_call",
        description = "Fetch full untruncated input/output and error details for one tool_call_id."
    )]
    pub async fn sessions_get_tool_call(
        &self,
        Parameters(request): Parameters<SessionsToolCallParams>,
    ) -> Result<Json<SessionsToolCallReceipt>, ErrorData> {
        let tool_call = self
            .service
            .get_tool_call(&request.session_id, &request.tool_call_id)
            .await
            .map_err(admin_error)?;
        let next_hint = format!(
            "Tool `{}` finished with status `{}`.",
            tool_call.tool_name, tool_call.status
        );
        Ok(Json(SessionsToolCallReceipt {
            tool_call,
            next_hint,
        }))
    }

    #[tool(
        name = "sessions_update",
        description = "Update a session's title, status (active/archived) or pin state."
    )]
    pub async fn sessions_update(
        &self,
        Parameters(request): Parameters<SessionsUpdateParams>,
    ) -> Result<Json<SessionsUpdateReceipt>, ErrorData> {
        let session = self
            .service
            .update_session(
                &request.session_id,
                SessionPatch {
                    title: request.title.as_deref(),
                    status: request.status,
                    pinned: request.pinned,
                },
            )
            .await
            .map_err(admin_error)?;
        let next_hint = format!("Session `{}` updated.", session.session_id);
        Ok(Json(SessionsUpdateReceipt { session, next_hint }))
    }

    #[tool(
        name = "sessions_action",
        description = "Perform a control action on a session: interrupt, retry, compact or delete."
    )]
    pub async fn sessions_action(
        &self,
        Parameters(request): Parameters<SessionsActionParams>,
    ) -> Result<Json<SessionsActionReceipt>, ErrorData> {
        let session = match request.action {
            SessionAdminAction::Interrupt => Some(
                self.service
                    .interrupt(&request.session_id, request.run_id.as_deref())
                    .await
                    .map_err(admin_error)?,
            ),
            SessionAdminAction::Retry => Some(
                self.service
                    .retry(&request.session_id)
                    .await
                    .map_err(admin_error)?,
            ),
            SessionAdminAction::Compact => Some(
                self.service
                    .compact(&request.session_id)
                    .await
                    .map_err(admin_error)?,
            ),
            SessionAdminAction::Delete => {
                self.service
                    .delete_session(&request.session_id)
                    .await
                    .map_err(admin_error)?;
                None
            }
        };
        let next_hint = match request.action {
            SessionAdminAction::Interrupt => {
                "Interrupt sent. Call sessions_inspect to verify idle/failed state.".to_string()
            }
            SessionAdminAction::Retry => {
                "Retry started. Watch Session-SSE or call sessions_inspect.".to_string()
            }
            SessionAdminAction::Compact => {
                "Pinned system prompt compacted to latest KB version.".to_string()
            }
            SessionAdminAction::Delete => {
                format!("Session `{}` deleted.", request.session_id)
            }
        };
        Ok(Json(SessionsActionReceipt {
            session_id: request.session_id,
            action: request.action,
            session,
            next_hint,
        }))
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
                Some(serde_json::json!({
                    "code": code,
                    "hint": hint,
                    "details": details,
                })),
            )
        }
        RuntimeError::SessionNotFound(_)
        | RuntimeError::RunNotFound(_)
        | RuntimeError::ToolCallNotFound(_)
        | RuntimeError::InvalidRequest(_) => ErrorData::invalid_params(
            error.to_string(),
            Some(serde_json::json!({"code": code, "hint": hint})),
        ),
        other => {
            tracing::warn!(error = %other, "admin MCP operation failed");
            ErrorData::internal_error(
                "internal runtime error; retry the request or inspect runtime logs",
                Some(serde_json::json!({"code": code, "hint": hint})),
            )
        }
    }
}
