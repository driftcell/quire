use super::*;

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, JsonSchema)]
pub struct KbDocumentRecord {
    pub doc_id: String,
    pub name: String,
    pub content: String,
    pub version: u64,
    pub updated_at: DateTime<Utc>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, JsonSchema)]
pub struct AgentDefinitionRecord {
    pub name: String,
    #[serde(default)]
    pub description: String,
    pub model: String,
    pub system_prompt: String,
    pub allowed_tools: Vec<String>,
    #[serde(default)]
    pub is_default: bool,
    pub version: u64,
    pub enabled: bool,
    pub updated_at: DateTime<Utc>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, JsonSchema)]
pub struct SkillResourceRecord {
    pub path: String,
    pub content: String,
    pub version: u64,
}

pub struct UpsertKbAgentCommand<'a> {
    pub name: &'a str,
    pub description: &'a str,
    pub model: &'a str,
    pub system_prompt: &'a str,
    pub allowed_tools: &'a [String],
    pub is_default: bool,
    pub enabled: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, JsonSchema)]
pub struct SkillDefinitionRecord {
    pub name: String,
    pub description: String,
    pub body: String,
    pub version: u64,
    pub enabled: bool,
    pub resources: Vec<SkillResourceRecord>,
    pub updated_at: DateTime<Utc>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum KbBranchStatus {
    Active,
    Draft,
    Archived,
}

impl KbBranchStatus {
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Active => "active",
            Self::Draft => "draft",
            Self::Archived => "archived",
        }
    }

    pub fn parse(value: &str) -> Option<Self> {
        match value {
            "active" => Some(Self::Active),
            "draft" => Some(Self::Draft),
            "archived" => Some(Self::Archived),
            _ => None,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
pub struct KbBranchRecord {
    pub name: String,
    pub description: String,
    pub status: KbBranchStatus,
    pub base_version: u64,
    pub head_version: u64,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
}

#[derive(
    Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize, JsonSchema,
)]
#[serde(rename_all = "snake_case")]
pub enum KbItemKind {
    #[serde(alias = "document", alias = "docs")]
    Doc,
    #[serde(alias = "agents")]
    Agent,
    #[serde(alias = "skills")]
    Skill,
}

impl KbItemKind {
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Doc => "doc",
            Self::Agent => "agent",
            Self::Skill => "skill",
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
pub struct KbItemSummary {
    pub item_kind: KbItemKind,
    pub item_key: String,
    pub version: u64,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub enabled: Option<bool>,
    pub updated_at: DateTime<Utc>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum KbDiffStatus {
    Added,
    Modified,
    Deleted,
    Conflict,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct KbItemDiff {
    pub item_kind: KbItemKind,
    pub item_key: String,
    pub status: KbDiffStatus,
    pub active_changed: bool,
    pub branch_changed: bool,
    pub conflict: bool,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub base_content: Option<Value>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub active_content: Option<Value>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub branch_content: Option<Value>,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
pub struct KbBranchDiff {
    pub branch: String,
    pub active_branch: String,
    pub base_version: u64,
    pub active_version: u64,
    pub branch_version: u64,
    pub changes: Vec<KbItemDiff>,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
pub struct KbConflictEntry {
    pub item_kind: KbItemKind,
    pub item_key: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub base_content: Option<Value>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub active_content: Option<Value>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub branch_content: Option<Value>,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
pub struct KbConflictReport {
    pub branch: String,
    pub active_branch: String,
    pub base_version: u64,
    pub active_version: u64,
    pub conflicts: Vec<KbConflictEntry>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum KbPromoteMode {
    FastForward,
    Merged,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
pub struct KbPromoteReport {
    pub branch: String,
    pub active_branch: String,
    pub mode: KbPromoteMode,
    pub previous_version: u64,
    pub version: u64,
    pub snapshot_version: u64,
    pub applied_items: usize,
    pub cache_invalidated: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
pub struct KbRollbackReport {
    pub active_branch: String,
    pub restored_from_version: u64,
    pub previous_version: u64,
    pub version: u64,
    pub snapshot_version: u64,
    pub applied_items: usize,
    pub cache_invalidated: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
pub struct KbSnapshotSummary {
    pub version: u64,
    pub branch: String,
    pub source: String,
    pub created_at: DateTime<Utc>,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub(crate) struct KbEntries {
    #[serde(default)]
    pub(crate) documents: Vec<KbDocumentRecord>,
    #[serde(default)]
    pub(crate) agents: Vec<AgentDefinitionRecord>,
    #[serde(default)]
    pub(crate) skills: Vec<SkillDefinitionRecord>,
}

#[derive(Debug, Clone)]
pub(crate) struct KbConflictMark {
    pub(crate) branch_head_version: u64,
}

#[derive(Serialize)]
struct KbDocumentContent<'a> {
    doc_id: &'a str,
    name: &'a str,
    content: &'a str,
}

#[derive(Serialize)]
struct KbAgentContent<'a> {
    name: &'a str,
    description: &'a str,
    model: &'a str,
    system_prompt: &'a str,
    allowed_tools: &'a [String],
    is_default: bool,
    enabled: bool,
}

#[derive(Serialize)]
struct KbSkillResourceContent<'a> {
    path: &'a str,
    content: &'a str,
}

#[derive(Serialize)]
struct KbSkillContent<'a> {
    name: &'a str,
    description: &'a str,
    body: &'a str,
    enabled: bool,
    resources: Vec<KbSkillResourceContent<'a>>,
}

#[derive(Debug, Clone)]
pub(crate) enum KbItemEntry {
    Document(KbDocumentRecord),
    Agent(AgentDefinitionRecord),
    Skill(SkillDefinitionRecord),
}

impl KbItemEntry {
    pub(crate) fn kind(&self) -> KbItemKind {
        match self {
            Self::Document(_) => KbItemKind::Doc,
            Self::Agent(_) => KbItemKind::Agent,
            Self::Skill(_) => KbItemKind::Skill,
        }
    }

    pub(crate) fn key(&self) -> &str {
        match self {
            Self::Document(record) => &record.doc_id,
            Self::Agent(record) => &record.name,
            Self::Skill(record) => &record.name,
        }
    }

    pub(crate) fn content(&self) -> Value {
        let serialized = match self {
            Self::Document(record) => serde_json::to_value(KbDocumentContent {
                doc_id: &record.doc_id,
                name: &record.name,
                content: &record.content,
            }),
            Self::Agent(record) => serde_json::to_value(KbAgentContent {
                name: &record.name,
                description: &record.description,
                model: &record.model,
                system_prompt: &record.system_prompt,
                allowed_tools: &record.allowed_tools,
                is_default: record.is_default,
                enabled: record.enabled,
            }),
            Self::Skill(record) => serde_json::to_value(KbSkillContent {
                name: &record.name,
                description: &record.description,
                body: &record.body,
                enabled: record.enabled,
                resources: record
                    .resources
                    .iter()
                    .map(|resource| KbSkillResourceContent {
                        path: &resource.path,
                        content: &resource.content,
                    })
                    .collect(),
            }),
        };
        serialized.unwrap_or_else(|error| {
            warn!(%error, "failed to serialize knowledge-base item content");
            Value::Null
        })
    }
}

#[derive(Debug, Clone, Default, Serialize, Deserialize, PartialEq, Eq)]
pub struct SeedReport {
    pub documents: usize,
    pub agents: usize,
    pub skills: usize,
    pub kb_version: u64,
}
