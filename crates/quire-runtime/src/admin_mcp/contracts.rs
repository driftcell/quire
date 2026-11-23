use super::*;

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
    pub(crate) fn from_record(record: RunRecord) -> Self {
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
