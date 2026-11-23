use super::*;

#[derive(Debug, Default, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct EmptyParams {}

#[derive(Debug, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct KbCreateBranchParams {
    /// New draft branch name (`1..=64` ASCII letters, digits, `'-'`, `'_'`, or `'.'`, starting with an alphanumeric character).
    #[serde(alias = "branch")]
    pub name: String,
    /// Optional human-readable description of what this draft branch changes or tests.
    #[serde(default)]
    pub description: String,
    /// Optional source branch to clone from. Omit to clone from the current active production branch.
    #[serde(default, alias = "base", alias = "source", alias = "from_branch")]
    pub from: Option<String>,
}

#[derive(Debug, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct KbBranchParams {
    /// Target branch name returned by `kb_list_branches` (must be a `draft` branch when calling `kb_promote_branch`).
    #[serde(alias = "branch")]
    pub name: String,
}

#[derive(Debug, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct KbRollbackParams {
    /// Immutable snapshot `version` from `kb_list_branches.recent_snapshots` to restore. Omit to restore the most recent earlier snapshot before the current active version.
    #[serde(default, alias = "snapshot_version", alias = "to_version")]
    pub version: Option<u64>,
}

#[derive(Debug, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct KbListItemsParams {
    /// Knowledge-base branch name to inspect (e.g. `"main"` or a draft branch name from `kb_list_branches`).
    pub branch: String,
    /// Optional item category filter: `"doc"`, `"agent"`, or `"skill"`. Omit to list all items on `branch`.
    #[serde(default)]
    pub item_kind: Option<KbItemKind>,
}

#[derive(Debug, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct KbReadItemParams {
    /// Knowledge-base branch name to read from.
    pub branch: String,
    /// Item category: `"doc"`, `"agent"`, or `"skill"`.
    #[serde(alias = "kind")]
    pub item_kind: KbItemKind,
    /// Exact item identifier from `kb_list_items` (`doc_id` for `"doc"`, `name` for `"agent"` and `"skill"`).
    #[serde(alias = "key")]
    pub item_key: String,
}

#[derive(Debug, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct KbUpsertDocParams {
    /// Target branch to edit (prefer an isolated `draft` branch; archived branches reject writes).
    pub branch: String,
    /// Stable document identifier (`1..=64` chars, e.g. `"system_instructions"` or `"compaction_prompt"`).
    #[serde(alias = "id")]
    pub doc_id: String,
    /// Human-readable document display name.
    pub name: String,
    /// Complete Markdown content for the knowledge-base document (replaces the existing document content on `branch`).
    pub content: String,
}

#[derive(Debug, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct KbUpsertAgentParams {
    /// Target branch to edit (prefer an isolated `draft` branch).
    pub branch: String,
    /// Unique agent identifier (`1..=64` chars, e.g. `"main"`, `"researcher"`, `"writer"`, `"reviewer"`).
    pub name: String,
    /// Concise delegation description shown to the primary orchestrator via `list_subagents` explaining when to spawn this subagent.
    #[serde(default)]
    pub description: String,
    /// Registered model route ID from `models_list` (e.g. `"openai/gpt-5"` or `"deepseek/deepseek-chat"`).
    pub model: String,
    /// Complete Markdown system prompt governing this agent's role and workflow.
    pub system_prompt: String,
    /// Allow-list of built-in and MCP tool names or patterns (`"*"`, `"apply_operations"`, `"server_id:*"`, `"server_id:tool_name"`). Note: `spawn_subagents` and `ask_user` are automatically stripped from child subagents at runtime.
    #[serde(default, alias = "tools")]
    pub allowed_tools: Vec<String>,
    /// Set `true` to make this the branch's default primary session agent (automatically clears `is_default` on all other agents in `branch`).
    #[serde(default)]
    pub is_default: bool,
    /// Whether this agent can be selected or spawned (`true` by default).
    #[serde(default = "default_enabled")]
    pub enabled: bool,
}

#[derive(Debug, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct SkillResourceParam {
    /// Relative path inside the skill bundle (e.g. `"templates/table.json"` or `"examples/chart.json"`; no leading `/` or `..` segments).
    pub path: String,
    /// Full UTF-8 text content of the bundled resource file.
    pub content: String,
}

#[derive(Debug, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct KbUpsertSkillParams {
    /// Target branch to edit (prefer an isolated `draft` branch).
    pub branch: String,
    /// Unique skill name (`1..=64` chars, loaded at runtime via `load_skill(name)`).
    pub name: String,
    /// Short one-line summary shown in the `<available_skills>` system prompt index telling agents when to load this skill.
    pub description: String,
    /// Complete Markdown instructions returned when an agent calls `load_skill(name)`.
    pub body: String,
    /// Whether the skill is advertised in `<available_skills>` and loadable (`true` by default).
    #[serde(default = "default_enabled")]
    pub enabled: bool,
    /// Complete list of bundled resource files for this skill (replaces ALL existing resources for `name` on `branch`; read existing resources with `kb_read_item` first if you want to preserve them).
    #[serde(default)]
    pub resources: Vec<SkillResourceParam>,
}

#[derive(Debug, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct ModelsUpsertProviderParams {
    /// Stable provider identifier (`1..=64` chars, e.g. `"deepseek"`, `"dashscope"`, `"anthropic"`).
    pub provider_id: String,
    /// Human-readable provider display name.
    pub name: String,
    /// Upstream wire protocol: `"openai_compatible"` or `"anthropic"`.
    pub protocol: ProviderProtocol,
    /// Base HTTP API endpoint URL (e.g. `"https://api.deepseek.com/v1"`).
    pub base_url: String,
    /// Credential reference: `"env:VAR_NAME"` (recommended), `"enc:..."`, or a raw API key when `RUNTIME_MASTER_KEY` is configured.
    pub api_key_ref: String,
    /// Whether the provider is enabled for model resolution (`true` by default).
    #[serde(default = "default_enabled")]
    pub enabled: bool,
}

#[derive(Debug, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct ModelsImportParams {
    /// Registered local `provider_id` that will own the imported model routes.
    pub provider_id: String,
    /// Optional upstream `models.dev` provider key to import from (defaults to `provider_id` when omitted).
    #[serde(default)]
    pub provider_filter: Option<String>,
    /// Optional explicit allow-list of upstream `model_id`s to import; omit to import all tool-capable models under the provider.
    #[serde(default)]
    pub model_ids: Option<Vec<String>>,
    /// Optional inline `models.dev` JSON catalog payload for offline/air-gapped imports; omit to fetch from `url` or the configured catalog endpoint.
    #[serde(default)]
    pub payload: Option<Value>,
    /// Optional one-off `models.dev` catalog URL override.
    #[serde(default)]
    pub url: Option<String>,
}

#[derive(Debug, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct SessionsListParams {
    /// Maximum number of session summaries to return (`1..=200`, defaults to `50`).
    #[serde(default)]
    pub limit: Option<i64>,
    /// Zero-based pagination offset (defaults to `0`).
    #[serde(default)]
    pub offset: Option<i64>,
}

#[derive(Debug, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct SessionsInspectParams {
    /// Target `session_id` from `sessions_list`.
    pub session_id: String,
}

#[derive(Debug, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct KbDeleteItemParams {
    /// Target branch to delete from. Omit ONLY when intentionally deleting from the current active production branch.
    #[serde(default)]
    pub branch: Option<String>,
    /// Item category to delete: `"doc"`, `"agent"`, or `"skill"`.
    #[serde(alias = "kind")]
    pub item_kind: KbItemKind,
    /// Exact item identifier (`doc_id` for `"doc"`, `name` for `"agent"` and `"skill"`).
    #[serde(alias = "key", alias = "id")]
    pub item_key: String,
}

#[derive(Debug, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct ModelIdParams {
    /// Registered model route `id` from `models_list` (e.g. `"deepseek/deepseek-chat"`).
    pub id: String,
}

#[derive(Debug, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct ProviderIdParams {
    /// Registered `provider_id` from `models_list_providers`.
    pub provider_id: String,
}

#[derive(Debug, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct SessionsToolCallParams {
    /// Session ID that owns the run trace.
    pub session_id: String,
    /// Exact `tool_call_id` from `sessions_traces` (`RunTraceToolItem.tool_call_id`) or live stream events.
    pub tool_call_id: String,
}

#[derive(Debug, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct SessionsUpdateParams {
    /// Target `session_id` to update.
    pub session_id: String,
    /// Optional new human-readable session title. Omit to keep the current title.
    #[serde(default)]
    pub title: Option<String>,
    /// Optional lifecycle status (`"active"` or `"archived"`). Omit to keep the current status.
    #[serde(default)]
    pub status: Option<SessionStatus>,
    /// Optional pin state (`true` pins the session to the top with the current timestamp; `false` unpins; omit to keep unchanged).
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
    /// Target `session_id`.
    pub session_id: String,
    /// Lifecycle action: `"interrupt"` (cancel active run), `"retry"` (re-run last failed/interrupted turn), `"compact"` (summarize history & refresh pinned KB prompt while idle), or `"delete"` (permanently remove idle session and traces).
    pub action: SessionAdminAction,
    /// Optional specific `run_id` guard when `action` is `"interrupt"`; omit to interrupt whichever run is currently active on the session.
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
    /// Registered MCP `server_id` from `mcp_list_servers`.
    pub server_id: String,
}

#[derive(Debug, Default, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct McpListToolsParams {
    /// Optional `server_id` filter; omit to list discovered tools across all registered MCP servers.
    #[serde(default)]
    pub server_id: Option<String>,
}

#[derive(Debug, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct McpSetToolEnabledParams {
    /// Registered MCP `server_id` that provides the tool.
    pub server_id: String,
    /// Exact `tool_name` returned by `mcp_list_tools`.
    pub tool_name: String,
    /// Global enable flag (`true` allows agents with matching `allowed_tools` to call it; `false` disables it globally).
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
