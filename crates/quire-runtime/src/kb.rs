//! Knowledge-base, agent-definition and skill management with Git-style branches, immutable
//! snapshots and moka caching.

use std::{
    collections::{BTreeMap, BTreeSet},
    path::Path,
    sync::Arc,
};

use chrono::{DateTime, Utc};
use moka::future::Cache;
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use sqlx::{PgConnection, Row};
use tracing::warn;

use crate::{
    error::{RuntimeError, RuntimeResult},
    store::RuntimeStore,
};

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
    pub model: String,
    pub system_prompt: String,
    pub allowed_tools: Vec<String>,
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
struct KbEntries {
    #[serde(default)]
    documents: Vec<KbDocumentRecord>,
    #[serde(default)]
    agents: Vec<AgentDefinitionRecord>,
    #[serde(default)]
    skills: Vec<SkillDefinitionRecord>,
}

#[derive(Debug, Clone)]
struct KbConflictMark {
    branch_head_version: u64,
}

#[derive(Debug, Clone)]
enum KbItemEntry {
    Document(KbDocumentRecord),
    Agent(AgentDefinitionRecord),
    Skill(SkillDefinitionRecord),
}

impl KbItemEntry {
    fn kind(&self) -> KbItemKind {
        match self {
            Self::Document(_) => KbItemKind::Doc,
            Self::Agent(_) => KbItemKind::Agent,
            Self::Skill(_) => KbItemKind::Skill,
        }
    }

    fn key(&self) -> &str {
        match self {
            Self::Document(record) => &record.doc_id,
            Self::Agent(record) => &record.name,
            Self::Skill(record) => &record.name,
        }
    }

    fn content(&self) -> Value {
        match self {
            Self::Document(record) => json!({
                "doc_id": record.doc_id,
                "name": record.name,
                "content": record.content,
            }),
            Self::Agent(record) => json!({
                "name": record.name,
                "model": record.model,
                "system_prompt": record.system_prompt,
                "allowed_tools": record.allowed_tools,
                "enabled": record.enabled,
            }),
            Self::Skill(record) => json!({
                "name": record.name,
                "description": record.description,
                "body": record.body,
                "enabled": record.enabled,
                "resources": record.resources.iter().map(|resource| json!({
                    "path": resource.path,
                    "content": resource.content,
                })).collect::<Vec<_>>(),
            }),
        }
    }
}

#[derive(Debug, Clone, Default, Serialize, Deserialize, PartialEq, Eq)]
pub struct SeedReport {
    pub documents: usize,
    pub agents: usize,
    pub skills: usize,
    pub kb_version: u64,
}

#[derive(Clone)]
pub struct KbService {
    store: RuntimeStore,
    prompt_cache: Cache<String, Arc<String>>,
}

#[derive(Debug, Default, Deserialize)]
struct Frontmatter {
    #[serde(default)]
    name: Option<String>,
    #[serde(default)]
    description: Option<String>,
    #[serde(default)]
    model: Option<String>,
    #[serde(default)]
    system_prompt: Option<String>,
    #[serde(default, alias = "tools")]
    allowed_tools: Vec<String>,
    #[serde(default)]
    enabled: Option<bool>,
}

impl KbService {
    pub fn new(store: RuntimeStore) -> Self {
        Self {
            store,
            prompt_cache: Cache::new(64),
        }
    }

    pub fn store(&self) -> &RuntimeStore {
        &self.store
    }

    pub async fn current_version(&self) -> RuntimeResult<u64> {
        self.store.kb_version().await
    }

    // ---------------------------------------------------------------------
    // Branches and snapshots
    // ---------------------------------------------------------------------

    pub async fn active_branch(&self) -> RuntimeResult<KbBranchRecord> {
        let row = sqlx::query(
            r#"SELECT name, description, status, base_version, head_version, created_at, updated_at
               FROM kb_branches WHERE status = 'active'
               ORDER BY name ASC LIMIT 1"#,
        )
        .fetch_optional(self.store.pool())
        .await?
        .ok_or_else(|| RuntimeError::Internal("no active knowledge-base branch".into()))?;
        row_to_branch(&row)
    }

    pub async fn list_branches(&self) -> RuntimeResult<Vec<KbBranchRecord>> {
        let rows = sqlx::query(
            r#"SELECT name, description, status, base_version, head_version, created_at, updated_at
               FROM kb_branches
               ORDER BY CASE status WHEN 'active' THEN 0 WHEN 'draft' THEN 1 ELSE 2 END,
                        updated_at DESC, name ASC"#,
        )
        .fetch_all(self.store.pool())
        .await?;
        rows.iter().map(row_to_branch).collect()
    }

    pub async fn get_branch(&self, name: &str) -> RuntimeResult<Option<KbBranchRecord>> {
        let row = sqlx::query(
            r#"SELECT name, description, status, base_version, head_version, created_at, updated_at
               FROM kb_branches WHERE name = $1"#,
        )
        .bind(name)
        .fetch_optional(self.store.pool())
        .await?;
        row.as_ref().map(row_to_branch).transpose()
    }

    pub async fn create_branch(
        &self,
        name: &str,
        description: &str,
        from: Option<&str>,
    ) -> RuntimeResult<KbBranchRecord> {
        validate_branch_name(name)?;
        let description = description.trim();
        let mut tx = self.store.pool().begin().await?;
        let source = match from {
            Some(source) => fetch_branch_for_update(&mut tx, source)
                .await?
                .ok_or_else(|| {
                    RuntimeError::InvalidRequest(format!("branch `{source}` does not exist"))
                })?,
            None => fetch_active_branch_for_update(&mut tx).await?,
        };
        if source.status != KbBranchStatus::Active {
            return Err(RuntimeError::InvalidRequest(
                "new branches can only be created from the active branch".into(),
            ));
        }
        let exists: bool =
            sqlx::query_scalar("SELECT EXISTS(SELECT 1 FROM kb_branches WHERE name = $1)")
                .bind(name)
                .fetch_one(&mut *tx)
                .await?;
        if exists {
            return Err(RuntimeError::InvalidRequest(format!(
                "branch `{name}` already exists"
            )));
        }
        let entries = fetch_entries(&mut tx, &source.name).await?;
        insert_snapshot(
            &mut tx,
            source.head_version,
            &source.name,
            "branch_base",
            &entries,
        )
        .await?;
        sqlx::query(
            r#"INSERT INTO kb_branches
               (name, description, status, base_version, head_version)
               VALUES ($1, $2, 'draft', $3, $3)"#,
        )
        .bind(name)
        .bind(description)
        .bind(i64::try_from(source.head_version).unwrap_or(i64::MAX))
        .execute(&mut *tx)
        .await?;
        copy_branch_entries(&mut tx, &source.name, name).await?;
        tx.commit().await?;
        self.get_branch(name)
            .await?
            .ok_or_else(|| RuntimeError::Internal("created branch vanished".into()))
    }

    pub async fn diff_branch(&self, name: &str) -> RuntimeResult<KbBranchDiff> {
        let branch = self.require_branch(name).await?;
        let active = self.active_branch().await?;
        let mut conn = self.store.pool().acquire().await?;
        let base = load_snapshot_entries(&mut conn, branch.base_version).await?;
        let active_entries = fetch_entries(&mut conn, &active.name).await?;
        let branch_entries = fetch_entries(&mut conn, &branch.name).await?;
        let base_map = item_map(&base);
        let active_map = item_map(&active_entries);
        let branch_map = item_map(&branch_entries);
        let mut keys = BTreeSet::new();
        keys.extend(base_map.keys().cloned());
        keys.extend(active_map.keys().cloned());
        keys.extend(branch_map.keys().cloned());
        let mut changes = Vec::new();
        for key in keys {
            let base_content = base_map.get(&key).map(KbItemEntry::content);
            let active_content = active_map.get(&key).map(KbItemEntry::content);
            let branch_content = branch_map.get(&key).map(KbItemEntry::content);
            let active_changed = active_content != base_content;
            let branch_changed = branch_content != base_content;
            let conflict = active_changed && branch_changed && active_content != branch_content;
            if branch_content == active_content {
                continue;
            }
            let status = if conflict {
                KbDiffStatus::Conflict
            } else if branch_content.is_none() {
                KbDiffStatus::Deleted
            } else if active_content.is_none() {
                KbDiffStatus::Added
            } else {
                KbDiffStatus::Modified
            };
            changes.push(KbItemDiff {
                item_kind: key.0,
                item_key: key.1,
                status,
                active_changed,
                branch_changed,
                conflict,
                base_content,
                active_content,
                branch_content,
            });
        }
        Ok(KbBranchDiff {
            branch: branch.name,
            active_branch: active.name,
            base_version: branch.base_version,
            active_version: active.head_version,
            branch_version: branch.head_version,
            changes,
        })
    }

    pub async fn promote_branch(&self, name: &str) -> RuntimeResult<KbPromoteReport> {
        let mut tx = self.store.pool().begin().await?;
        let active = fetch_active_branch_for_update(&mut tx).await?;
        let branch = fetch_branch_for_update(&mut tx, name)
            .await?
            .ok_or_else(|| {
                RuntimeError::InvalidRequest(format!("branch `{name}` does not exist"))
            })?;
        if branch.name == active.name {
            return Err(RuntimeError::InvalidRequest(
                "the active branch cannot be promoted onto itself".into(),
            ));
        }
        if branch.status != KbBranchStatus::Draft {
            return Err(RuntimeError::InvalidRequest(format!(
                "only draft branches can be promoted; `{}` is {}",
                branch.name,
                branch.status.as_str()
            )));
        }
        let active_entries = fetch_entries(&mut tx, &active.name).await?;
        let branch_entries = fetch_entries(&mut tx, &branch.name).await?;
        let conflict_mark = fetch_conflict_mark(&mut tx, &branch.name).await?;
        let mut rebased = false;
        if let Some(mark) = &conflict_mark
            && branch.head_version > mark.branch_head_version
        {
            // The administrator edited the draft after seeing the conflict. Treat those edits as
            // the resolution and make the current active snapshot the new merge base.
            insert_snapshot(
                &mut tx,
                active.head_version,
                &active.name,
                "conflict_rebase",
                &active_entries,
            )
            .await?;
            sqlx::query(
                "UPDATE kb_branches SET base_version = $2, updated_at = NOW() WHERE name = $1",
            )
            .bind(&branch.name)
            .bind(i64::try_from(active.head_version).unwrap_or(i64::MAX))
            .execute(&mut *tx)
            .await?;
            rebased = true;
        }
        let base = if rebased {
            active_entries.clone()
        } else {
            load_snapshot_entries(&mut tx, branch.base_version).await?
        };
        let (merged, conflicts) = merge_entries(&base, &active_entries, &branch_entries);
        if !conflicts.is_empty() {
            upsert_conflict_mark(
                &mut tx,
                &branch.name,
                active.head_version,
                branch.head_version,
            )
            .await?;
            let report = KbConflictReport {
                branch: branch.name,
                active_branch: active.name,
                base_version: branch.base_version,
                active_version: active.head_version,
                conflicts,
            };
            tx.commit().await?;
            return Err(RuntimeError::KbConflict(Box::new(report)));
        }
        let applied_items = apply_entries(&mut tx, &active.name, &merged).await?;
        let previous_version = active.head_version;
        let mode = if rebased || active.head_version != branch.base_version {
            KbPromoteMode::Merged
        } else {
            KbPromoteMode::FastForward
        };
        let version = bump_version(&mut tx).await?;
        sqlx::query("UPDATE kb_branches SET head_version = $2, updated_at = NOW() WHERE name = $1")
            .bind(&active.name)
            .bind(i64::try_from(version).unwrap_or(i64::MAX))
            .execute(&mut *tx)
            .await?;
        sqlx::query(
            r#"UPDATE kb_branches SET status = 'archived', head_version = $2, updated_at = NOW()
               WHERE name = $1"#,
        )
        .bind(&branch.name)
        .bind(i64::try_from(version).unwrap_or(i64::MAX))
        .execute(&mut *tx)
        .await?;
        delete_conflict_mark(&mut tx, &branch.name).await?;
        insert_snapshot(&mut tx, version, &active.name, "promote", &merged).await?;
        tx.commit().await?;
        self.after_active_commit(version).await;
        Ok(KbPromoteReport {
            branch: branch.name,
            active_branch: active.name,
            mode,
            previous_version,
            version,
            snapshot_version: version,
            applied_items,
            cache_invalidated: true,
        })
    }

    pub async fn rollback_active(&self, version: Option<u64>) -> RuntimeResult<KbRollbackReport> {
        let mut tx = self.store.pool().begin().await?;
        let active = fetch_active_branch_for_update(&mut tx).await?;
        let target_version = match version {
            Some(target) => {
                if target >= active.head_version {
                    return Err(RuntimeError::InvalidRequest(format!(
                        "rollback target version {target} must predate the active version {}",
                        active.head_version
                    )));
                }
                let exists: bool = sqlx::query_scalar(
                    "SELECT EXISTS(SELECT 1 FROM kb_snapshots WHERE version = $1)",
                )
                .bind(i64::try_from(target).unwrap_or(i64::MAX))
                .fetch_one(&mut *tx)
                .await?;
                if !exists {
                    return Err(RuntimeError::InvalidRequest(format!(
                        "snapshot version {target} does not exist"
                    )));
                }
                target
            }
            None => {
                let target = sqlx::query_scalar::<_, i64>(
                    r#"SELECT version FROM kb_snapshots
                       WHERE version < $1 ORDER BY version DESC LIMIT 1"#,
                )
                .bind(i64::try_from(active.head_version).unwrap_or(i64::MAX))
                .fetch_optional(&mut *tx)
                .await?
                .ok_or_else(|| {
                    RuntimeError::InvalidRequest(
                        "no earlier snapshot is available to roll back to".into(),
                    )
                })?;
                u64::try_from(target).unwrap_or(0)
            }
        };
        let snapshot = load_snapshot_entries(&mut tx, target_version).await?;
        let applied_items = apply_entries(&mut tx, &active.name, &snapshot).await?;
        let previous_version = active.head_version;
        let new_version = bump_version(&mut tx).await?;
        sqlx::query("UPDATE kb_branches SET head_version = $2, updated_at = NOW() WHERE name = $1")
            .bind(&active.name)
            .bind(i64::try_from(new_version).unwrap_or(i64::MAX))
            .execute(&mut *tx)
            .await?;
        insert_snapshot(&mut tx, new_version, &active.name, "rollback", &snapshot).await?;
        tx.commit().await?;
        self.after_active_commit(new_version).await;
        Ok(KbRollbackReport {
            active_branch: active.name,
            restored_from_version: target_version,
            previous_version,
            version: new_version,
            snapshot_version: new_version,
            applied_items,
            cache_invalidated: true,
        })
    }

    pub async fn list_snapshots(&self, limit: i64) -> RuntimeResult<Vec<KbSnapshotSummary>> {
        let rows = sqlx::query(
            r#"SELECT version, branch, source, created_at FROM kb_snapshots
               ORDER BY version DESC LIMIT $1"#,
        )
        .bind(limit.clamp(1, 200))
        .fetch_all(self.store.pool())
        .await?;
        rows.iter()
            .map(|row| {
                let version: i64 = row.try_get("version")?;
                Ok(KbSnapshotSummary {
                    version: u64::try_from(version).unwrap_or(0),
                    branch: row.try_get("branch")?,
                    source: row.try_get("source")?,
                    created_at: row.try_get("created_at")?,
                })
            })
            .collect()
    }

    /// Resolve the branch a new session should use and reject archived branches.
    pub async fn session_branch(&self, branch: Option<&str>) -> RuntimeResult<String> {
        let branch = self.resolve_branch(branch).await?;
        if branch.status == KbBranchStatus::Archived {
            return Err(RuntimeError::InvalidRequest(format!(
                "branch `{}` is archived and cannot back a new session",
                branch.name
            )));
        }
        Ok(branch.name)
    }

    pub async fn list_items(&self, branch: &str) -> RuntimeResult<Vec<KbItemSummary>> {
        let branch = self.resolve_branch(Some(branch)).await?.name;
        let mut items = Vec::new();
        for document in self.list_documents_on(Some(&branch)).await? {
            items.push(KbItemSummary {
                item_kind: KbItemKind::Doc,
                item_key: document.doc_id,
                version: document.version,
                enabled: None,
                updated_at: document.updated_at,
            });
        }
        for agent in self.list_agents_on(Some(&branch)).await? {
            items.push(KbItemSummary {
                item_kind: KbItemKind::Agent,
                item_key: agent.name,
                version: agent.version,
                enabled: Some(agent.enabled),
                updated_at: agent.updated_at,
            });
        }
        for skill in self.list_skills_on(Some(&branch)).await? {
            items.push(KbItemSummary {
                item_kind: KbItemKind::Skill,
                item_key: skill.name,
                version: skill.version,
                enabled: Some(skill.enabled),
                updated_at: skill.updated_at,
            });
        }
        items.sort_by(|left, right| {
            (left.item_kind, &left.item_key).cmp(&(right.item_kind, &right.item_key))
        });
        Ok(items)
    }

    pub async fn read_item(
        &self,
        branch: &str,
        kind: KbItemKind,
        key: &str,
    ) -> RuntimeResult<Option<Value>> {
        let branch = self.resolve_branch(Some(branch)).await?.name;
        match kind {
            KbItemKind::Doc => {
                let row = sqlx::query(
                    r#"SELECT doc_id, name, content, version, updated_at FROM kb_documents
                       WHERE branch = $1 AND doc_id = $2"#,
                )
                .bind(&branch)
                .bind(key)
                .fetch_optional(self.store.pool())
                .await?;
                row.as_ref()
                    .map(row_to_document)
                    .transpose()
                    .map(|record| record.map(|record| KbItemEntry::Document(record).content()))
            }
            KbItemKind::Agent => Ok(self
                .get_agent_on(Some(&branch), key)
                .await?
                .map(|record| KbItemEntry::Agent(record).content())),
            KbItemKind::Skill => Ok(self
                .get_skill_on(Some(&branch), key)
                .await?
                .map(|record| KbItemEntry::Skill(record).content())),
        }
    }

    // ---------------------------------------------------------------------
    // System prompt and seeding
    // ---------------------------------------------------------------------

    /// Build (and cache) the frozen system prompt from KB documents and skill summaries.
    pub async fn system_prompt(&self) -> RuntimeResult<Arc<String>> {
        self.system_prompt_for(None).await
    }

    pub async fn system_prompt_for(&self, branch: Option<&str>) -> RuntimeResult<Arc<String>> {
        let branch = self.resolve_branch(branch).await?.name;
        let cache_key = format!("system_prompt:{branch}");
        if let Some(cached) = self.prompt_cache.get(&cache_key).await {
            return Ok(cached);
        }
        let docs = self.list_documents_on(Some(&branch)).await?;
        let skills = self.list_skills_on(Some(&branch)).await?;
        let mut prompt = String::new();
        for doc in &docs {
            if !prompt.is_empty() {
                prompt.push_str("\n\n");
            }
            prompt.push_str(&doc.content);
        }
        if prompt.trim().is_empty() {
            prompt.push_str(
                "You are the Quire report agent. Use the provided MCP tools to author, review and lint reports.",
            );
        }
        prompt.push_str("\n\n<available_skills>\n");
        for skill in skills.iter().filter(|skill| skill.enabled) {
            prompt.push_str("- ");
            prompt.push_str(&skill.name);
            prompt.push_str(": ");
            prompt.push_str(skill.description.trim());
            prompt.push('\n');
        }
        prompt.push_str("</available_skills>\n");
        let prompt = Arc::new(prompt);
        self.prompt_cache.insert(cache_key, prompt.clone()).await;
        Ok(prompt)
    }

    pub fn spawn_invalidation_listener(&self) -> tokio::task::JoinHandle<()> {
        let cache = self.prompt_cache.clone();
        let bus = self.store.bus().clone();
        tokio::spawn(async move {
            let Ok(mut subscriber) = bus.subscribe_kb_invalidation().await else {
                return;
            };
            while subscriber.next_invalidation().await.is_some() {
                cache.invalidate_all();
                cache.run_pending_tasks().await;
            }
        })
    }

    pub async fn seed(&self, workspace: &Path, default_model: &str) -> RuntimeResult<SeedReport> {
        let branch = self.active_branch().await?.name;
        let mut report = SeedReport::default();
        let document_count: i64 =
            sqlx::query_scalar("SELECT COUNT(*)::bigint FROM kb_documents WHERE branch = $1")
                .bind(&branch)
                .fetch_one(self.store.pool())
                .await?;
        if document_count == 0 {
            // The main system prompt is the AGENTS.md KB document. Seed it from the
            // report editor definition so `agents/main.md` remains the single source.
            let main_agent = workspace.join("agents").join("main.md");
            if main_agent.is_file() {
                let raw = tokio::fs::read_to_string(&main_agent).await?;
                let (frontmatter, body) = parse_frontmatter(&raw)?;
                let content = frontmatter
                    .as_ref()
                    .and_then(|value| value.system_prompt.clone())
                    .filter(|value| !value.trim().is_empty())
                    .unwrap_or(body);
                self.insert_document_raw(&branch, "agents-md", "AGENTS.md", &content)
                    .await?;
                report.documents += 1;
            }
        }

        let agent_count: i64 =
            sqlx::query_scalar("SELECT COUNT(*)::bigint FROM agent_definitions WHERE branch = $1")
                .bind(&branch)
                .fetch_one(self.store.pool())
                .await?;
        if agent_count == 0 {
            let agents_dir = workspace.join("agents");
            if agents_dir.is_dir() {
                for entry in std::fs::read_dir(&agents_dir)? {
                    let entry = entry?;
                    let path = entry.path();
                    if !is_markdown(&path) {
                        continue;
                    }
                    let raw = tokio::fs::read_to_string(&path).await?;
                    let (frontmatter, body) = parse_frontmatter(&raw)?;
                    let fallback = path
                        .file_stem()
                        .and_then(|stem| stem.to_str())
                        .unwrap_or("agent")
                        .to_string();
                    let name = frontmatter
                        .as_ref()
                        .and_then(|value| value.name.clone())
                        .unwrap_or(fallback);
                    let model = frontmatter
                        .as_ref()
                        .and_then(|value| value.model.clone())
                        .unwrap_or_else(|| default_model.to_string());
                    let system_prompt = frontmatter
                        .as_ref()
                        .and_then(|value| value.system_prompt.clone())
                        .filter(|value| !value.trim().is_empty())
                        .unwrap_or(body);
                    let allowed_tools = frontmatter
                        .as_ref()
                        .map(|value| value.allowed_tools.clone())
                        .unwrap_or_default();
                    let enabled = frontmatter
                        .as_ref()
                        .and_then(|value| value.enabled)
                        .unwrap_or(true);
                    self.insert_agent_raw(
                        &branch,
                        &name,
                        &model,
                        &system_prompt,
                        &allowed_tools,
                        enabled,
                    )
                    .await?;
                    report.agents += 1;
                }
            }
        }

        let skill_count: i64 =
            sqlx::query_scalar("SELECT COUNT(*)::bigint FROM skill_definitions WHERE branch = $1")
                .bind(&branch)
                .fetch_one(self.store.pool())
                .await?;
        if skill_count == 0 {
            let skills_dir = workspace.join("skills");
            if skills_dir.is_dir() {
                for entry in std::fs::read_dir(&skills_dir)? {
                    let entry = entry?;
                    let path = entry.path();
                    if !path.is_dir() {
                        continue;
                    }
                    let manifest = path.join("SKILL.md");
                    if !manifest.is_file() {
                        continue;
                    }
                    let raw = tokio::fs::read_to_string(&manifest).await?;
                    let (frontmatter, body) = parse_frontmatter(&raw)?;
                    let fallback = path
                        .file_name()
                        .and_then(|name| name.to_str())
                        .unwrap_or("skill")
                        .to_string();
                    let name = frontmatter
                        .as_ref()
                        .and_then(|value| value.name.clone())
                        .unwrap_or(fallback);
                    let description = frontmatter
                        .as_ref()
                        .and_then(|value| value.description.clone())
                        .unwrap_or_default();
                    let resources = read_skill_resources(&path)?;
                    self.insert_skill_raw(&branch, &name, &description, &body, true, &resources)
                        .await?;
                    report.skills += 1;
                }
            }
        }

        if report.documents + report.agents + report.skills > 0 {
            report.kb_version = self
                .after_kb_mutation_on(&branch, KbBranchStatus::Active)
                .await?;
        } else {
            report.kb_version = self.store.kb_version().await?;
        }
        Ok(report)
    }

    // ---------------------------------------------------------------------
    // Documents
    // ---------------------------------------------------------------------

    pub async fn list_documents(&self) -> RuntimeResult<Vec<KbDocumentRecord>> {
        self.list_documents_on(None).await
    }

    pub async fn list_documents_on(
        &self,
        branch: Option<&str>,
    ) -> RuntimeResult<Vec<KbDocumentRecord>> {
        let branch = self.resolve_branch(branch).await?.name;
        let rows = sqlx::query(
            r#"SELECT doc_id, name, content, version, updated_at FROM kb_documents
               WHERE branch = $1 ORDER BY name ASC"#,
        )
        .bind(&branch)
        .fetch_all(self.store.pool())
        .await?;
        rows.iter().map(row_to_document).collect()
    }

    pub async fn upsert_document(
        &self,
        doc_id: &str,
        name: &str,
        content: &str,
    ) -> RuntimeResult<KbDocumentRecord> {
        let branch = self.active_branch().await?.name;
        self.upsert_document_on(&branch, doc_id, name, content)
            .await
    }

    pub async fn upsert_document_on(
        &self,
        branch: &str,
        doc_id: &str,
        name: &str,
        content: &str,
    ) -> RuntimeResult<KbDocumentRecord> {
        let branch_record = self.mutable_branch(branch).await?;
        sqlx::query(
            r#"INSERT INTO kb_documents (branch, doc_id, name, content, version)
               VALUES ($1, $2, $3, $4, 1)
               ON CONFLICT (branch, doc_id) DO UPDATE
               SET name = EXCLUDED.name, content = EXCLUDED.content,
                   version = kb_documents.version + 1, updated_at = NOW()"#,
        )
        .bind(&branch_record.name)
        .bind(doc_id)
        .bind(name)
        .bind(content)
        .execute(self.store.pool())
        .await?;
        self.after_kb_mutation_on(&branch_record.name, branch_record.status)
            .await?;
        let row = sqlx::query(
            r#"SELECT doc_id, name, content, version, updated_at FROM kb_documents
               WHERE branch = $1 AND doc_id = $2"#,
        )
        .bind(&branch_record.name)
        .bind(doc_id)
        .fetch_one(self.store.pool())
        .await?;
        row_to_document(&row)
    }

    pub async fn delete_document(&self, doc_id: &str) -> RuntimeResult<()> {
        let branch = self.active_branch().await?.name;
        self.delete_document_on(&branch, doc_id).await
    }

    pub async fn delete_document_on(&self, branch: &str, doc_id: &str) -> RuntimeResult<()> {
        let branch_record = self.mutable_branch(branch).await?;
        sqlx::query("DELETE FROM kb_documents WHERE branch = $1 AND doc_id = $2")
            .bind(&branch_record.name)
            .bind(doc_id)
            .execute(self.store.pool())
            .await?;
        self.after_kb_mutation_on(&branch_record.name, branch_record.status)
            .await?;
        Ok(())
    }

    // ---------------------------------------------------------------------
    // Agents
    // ---------------------------------------------------------------------

    pub async fn list_agents(&self) -> RuntimeResult<Vec<AgentDefinitionRecord>> {
        self.list_agents_on(None).await
    }

    pub async fn list_agents_on(
        &self,
        branch: Option<&str>,
    ) -> RuntimeResult<Vec<AgentDefinitionRecord>> {
        let branch = self.resolve_branch(branch).await?.name;
        let rows = sqlx::query(
            r#"SELECT name, model, system_prompt, allowed_tools, version, enabled, updated_at
               FROM agent_definitions WHERE branch = $1 ORDER BY name ASC"#,
        )
        .bind(&branch)
        .fetch_all(self.store.pool())
        .await?;
        rows.iter().map(row_to_agent).collect()
    }

    pub async fn get_agent(&self, name: &str) -> RuntimeResult<Option<AgentDefinitionRecord>> {
        self.get_agent_on(None, name).await
    }

    pub async fn get_agent_on(
        &self,
        branch: Option<&str>,
        name: &str,
    ) -> RuntimeResult<Option<AgentDefinitionRecord>> {
        let branch = self.resolve_branch(branch).await?.name;
        let row = sqlx::query(
            r#"SELECT name, model, system_prompt, allowed_tools, version, enabled, updated_at
               FROM agent_definitions WHERE branch = $1 AND name = $2"#,
        )
        .bind(&branch)
        .bind(name)
        .fetch_optional(self.store.pool())
        .await?;
        row.as_ref().map(row_to_agent).transpose()
    }

    pub async fn upsert_agent(
        &self,
        name: &str,
        model: &str,
        system_prompt: &str,
        allowed_tools: &[String],
        enabled: bool,
    ) -> RuntimeResult<AgentDefinitionRecord> {
        let branch = self.active_branch().await?.name;
        self.upsert_agent_on(&branch, name, model, system_prompt, allowed_tools, enabled)
            .await
    }

    pub async fn upsert_agent_on(
        &self,
        branch: &str,
        name: &str,
        model: &str,
        system_prompt: &str,
        allowed_tools: &[String],
        enabled: bool,
    ) -> RuntimeResult<AgentDefinitionRecord> {
        let branch_record = self.mutable_branch(branch).await?;
        let tools = serde_json::to_value(allowed_tools)?;
        sqlx::query(
            r#"INSERT INTO agent_definitions
               (branch, name, model, system_prompt, allowed_tools, version, enabled)
               VALUES ($1, $2, $3, $4, $5, 1, $6)
               ON CONFLICT (branch, name) DO UPDATE
               SET model = EXCLUDED.model, system_prompt = EXCLUDED.system_prompt,
                   allowed_tools = EXCLUDED.allowed_tools, enabled = EXCLUDED.enabled,
                   version = agent_definitions.version + 1, updated_at = NOW()"#,
        )
        .bind(&branch_record.name)
        .bind(name)
        .bind(model)
        .bind(system_prompt)
        .bind(tools)
        .bind(enabled)
        .execute(self.store.pool())
        .await?;
        self.after_kb_mutation_on(&branch_record.name, branch_record.status)
            .await?;
        self.get_agent_on(Some(&branch_record.name), name)
            .await?
            .ok_or_else(|| RuntimeError::Internal("agent definition vanished".into()))
    }

    pub async fn delete_agent(&self, name: &str) -> RuntimeResult<()> {
        let branch = self.active_branch().await?.name;
        self.delete_agent_on(&branch, name).await
    }

    pub async fn delete_agent_on(&self, branch: &str, name: &str) -> RuntimeResult<()> {
        let branch_record = self.mutable_branch(branch).await?;
        sqlx::query("DELETE FROM agent_definitions WHERE branch = $1 AND name = $2")
            .bind(&branch_record.name)
            .bind(name)
            .execute(self.store.pool())
            .await?;
        self.after_kb_mutation_on(&branch_record.name, branch_record.status)
            .await?;
        Ok(())
    }

    // ---------------------------------------------------------------------
    // Skills
    // ---------------------------------------------------------------------

    pub async fn list_skills(&self) -> RuntimeResult<Vec<SkillDefinitionRecord>> {
        self.list_skills_on(None).await
    }

    pub async fn list_skills_on(
        &self,
        branch: Option<&str>,
    ) -> RuntimeResult<Vec<SkillDefinitionRecord>> {
        let branch = self.resolve_branch(branch).await?.name;
        let rows = sqlx::query(
            r#"SELECT name, description, body, version, enabled, updated_at
               FROM skill_definitions WHERE branch = $1 ORDER BY name ASC"#,
        )
        .bind(&branch)
        .fetch_all(self.store.pool())
        .await?;
        let mut skills = Vec::new();
        for row in rows {
            let name: String = row.try_get("name")?;
            let version: i64 = row.try_get("version")?;
            let resources = self.list_skill_resources(&branch, &name).await?;
            skills.push(SkillDefinitionRecord {
                name,
                description: row.try_get("description")?,
                body: row.try_get("body")?,
                version: u64::try_from(version).unwrap_or(0),
                enabled: row.try_get("enabled")?,
                resources,
                updated_at: row.try_get("updated_at")?,
            });
        }
        Ok(skills)
    }

    pub async fn get_skill(&self, name: &str) -> RuntimeResult<Option<SkillDefinitionRecord>> {
        self.get_skill_on(None, name).await
    }

    pub async fn get_skill_on(
        &self,
        branch: Option<&str>,
        name: &str,
    ) -> RuntimeResult<Option<SkillDefinitionRecord>> {
        Ok(self
            .list_skills_on(branch)
            .await?
            .into_iter()
            .find(|skill| skill.name == name))
    }

    pub async fn upsert_skill(
        &self,
        name: &str,
        description: &str,
        body: &str,
        enabled: bool,
        resources: &[(String, String)],
    ) -> RuntimeResult<SkillDefinitionRecord> {
        let branch = self.active_branch().await?.name;
        self.upsert_skill_on(&branch, name, description, body, enabled, resources)
            .await
    }

    pub async fn upsert_skill_on(
        &self,
        branch: &str,
        name: &str,
        description: &str,
        body: &str,
        enabled: bool,
        resources: &[(String, String)],
    ) -> RuntimeResult<SkillDefinitionRecord> {
        let branch_record = self.mutable_branch(branch).await?;
        let mut tx = self.store.pool().begin().await?;
        sqlx::query(
            r#"INSERT INTO skill_definitions (branch, name, description, body, version, enabled)
               VALUES ($1, $2, $3, $4, 1, $5)
               ON CONFLICT (branch, name) DO UPDATE
               SET description = EXCLUDED.description, body = EXCLUDED.body,
                   enabled = EXCLUDED.enabled, version = skill_definitions.version + 1,
                   updated_at = NOW()"#,
        )
        .bind(&branch_record.name)
        .bind(name)
        .bind(description)
        .bind(body)
        .bind(enabled)
        .execute(&mut *tx)
        .await?;
        let version: i64 = sqlx::query_scalar(
            "SELECT version FROM skill_definitions WHERE branch = $1 AND name = $2",
        )
        .bind(&branch_record.name)
        .bind(name)
        .fetch_one(&mut *tx)
        .await?;
        sqlx::query("DELETE FROM skill_resources WHERE branch = $1 AND skill_name = $2")
            .bind(&branch_record.name)
            .bind(name)
            .execute(&mut *tx)
            .await?;
        for (path, content) in resources {
            sqlx::query(
                r#"INSERT INTO skill_resources (branch, skill_name, path, content, version)
                   VALUES ($1, $2, $3, $4, $5)"#,
            )
            .bind(&branch_record.name)
            .bind(name)
            .bind(path)
            .bind(content)
            .bind(version)
            .execute(&mut *tx)
            .await?;
        }
        tx.commit().await?;
        self.after_kb_mutation_on(&branch_record.name, branch_record.status)
            .await?;
        self.get_skill_on(Some(&branch_record.name), name)
            .await?
            .ok_or_else(|| RuntimeError::Internal("skill definition vanished".into()))
    }

    pub async fn delete_skill(&self, name: &str) -> RuntimeResult<()> {
        let branch = self.active_branch().await?.name;
        self.delete_skill_on(&branch, name).await
    }

    pub async fn delete_skill_on(&self, branch: &str, name: &str) -> RuntimeResult<()> {
        let branch_record = self.mutable_branch(branch).await?;
        sqlx::query("DELETE FROM skill_definitions WHERE branch = $1 AND name = $2")
            .bind(&branch_record.name)
            .bind(name)
            .execute(self.store.pool())
            .await?;
        self.after_kb_mutation_on(&branch_record.name, branch_record.status)
            .await?;
        Ok(())
    }

    async fn list_skill_resources(
        &self,
        branch: &str,
        name: &str,
    ) -> RuntimeResult<Vec<SkillResourceRecord>> {
        let rows = sqlx::query(
            r#"SELECT path, content, version FROM skill_resources
               WHERE branch = $1 AND skill_name = $2 ORDER BY path ASC"#,
        )
        .bind(branch)
        .bind(name)
        .fetch_all(self.store.pool())
        .await?;
        rows.iter()
            .map(|row| {
                let version: i64 = row.try_get("version")?;
                Ok(SkillResourceRecord {
                    path: row.try_get("path")?,
                    content: row.try_get("content")?,
                    version: u64::try_from(version).unwrap_or(0),
                })
            })
            .collect()
    }

    // ---------------------------------------------------------------------
    // Mutation bookkeeping and internal helpers
    // ---------------------------------------------------------------------

    async fn resolve_branch(&self, branch: Option<&str>) -> RuntimeResult<KbBranchRecord> {
        match branch {
            Some(name) => self.require_branch(name.trim()).await,
            None => self.active_branch().await,
        }
    }

    async fn require_branch(&self, name: &str) -> RuntimeResult<KbBranchRecord> {
        if name.is_empty() {
            return Err(RuntimeError::InvalidRequest(
                "branch name must not be empty".into(),
            ));
        }
        self.get_branch(name)
            .await?
            .ok_or_else(|| RuntimeError::InvalidRequest(format!("branch `{name}` does not exist")))
    }

    async fn mutable_branch(&self, name: &str) -> RuntimeResult<KbBranchRecord> {
        let branch = self.require_branch(name).await?;
        if branch.status == KbBranchStatus::Archived {
            return Err(RuntimeError::InvalidRequest(format!(
                "branch `{name}` is archived and read-only"
            )));
        }
        Ok(branch)
    }

    async fn after_kb_mutation_on(
        &self,
        branch: &str,
        status: KbBranchStatus,
    ) -> RuntimeResult<u64> {
        let version = match status {
            KbBranchStatus::Active => {
                let version = self.store.bump_kb_version().await?;
                sqlx::query(
                    r#"UPDATE kb_branches SET head_version = $2, updated_at = NOW()
                       WHERE name = $1 AND status = 'active'"#,
                )
                .bind(branch)
                .bind(i64::try_from(version).unwrap_or(i64::MAX))
                .execute(self.store.pool())
                .await?;
                version
            }
            KbBranchStatus::Draft => {
                let head: i64 = sqlx::query_scalar(
                    r#"UPDATE kb_branches SET head_version = head_version + 1, updated_at = NOW()
                       WHERE name = $1 AND status = 'draft' RETURNING head_version"#,
                )
                .bind(branch)
                .fetch_optional(self.store.pool())
                .await?
                .ok_or_else(|| {
                    RuntimeError::InvalidRequest(format!(
                        "branch `{branch}` is no longer a mutable draft"
                    ))
                })?;
                u64::try_from(head).unwrap_or(0)
            }
            KbBranchStatus::Archived => {
                return Err(RuntimeError::InvalidRequest(format!(
                    "branch `{branch}` is archived and read-only"
                )));
            }
        };
        self.invalidate_prompt_cache().await;
        Ok(version)
    }

    async fn after_active_commit(&self, version: u64) {
        self.invalidate_prompt_cache().await;
        if let Err(error) = self.store.bus().publish_kb_invalidation(version).await {
            warn!(%error, version, "failed to publish KB invalidation after branch release");
        }
    }

    async fn invalidate_prompt_cache(&self) {
        self.prompt_cache.invalidate_all();
        self.prompt_cache.run_pending_tasks().await;
    }

    async fn insert_document_raw(
        &self,
        branch: &str,
        doc_id: &str,
        name: &str,
        content: &str,
    ) -> RuntimeResult<()> {
        sqlx::query(
            r#"INSERT INTO kb_documents (branch, doc_id, name, content, version)
               VALUES ($1, $2, $3, $4, 1)
               ON CONFLICT (branch, doc_id) DO NOTHING"#,
        )
        .bind(branch)
        .bind(doc_id)
        .bind(name)
        .bind(content)
        .execute(self.store.pool())
        .await?;
        Ok(())
    }

    async fn insert_agent_raw(
        &self,
        branch: &str,
        name: &str,
        model: &str,
        system_prompt: &str,
        allowed_tools: &[String],
        enabled: bool,
    ) -> RuntimeResult<()> {
        sqlx::query(
            r#"INSERT INTO agent_definitions
               (branch, name, model, system_prompt, allowed_tools, version, enabled)
               VALUES ($1, $2, $3, $4, $5, 1, $6)
               ON CONFLICT (branch, name) DO NOTHING"#,
        )
        .bind(branch)
        .bind(name)
        .bind(model)
        .bind(system_prompt)
        .bind(serde_json::to_value(allowed_tools)?)
        .bind(enabled)
        .execute(self.store.pool())
        .await?;
        Ok(())
    }

    async fn insert_skill_raw(
        &self,
        branch: &str,
        name: &str,
        description: &str,
        body: &str,
        enabled: bool,
        resources: &[(String, String)],
    ) -> RuntimeResult<()> {
        let mut tx = self.store.pool().begin().await?;
        sqlx::query(
            r#"INSERT INTO skill_definitions (branch, name, description, body, version, enabled)
               VALUES ($1, $2, $3, $4, 1, $5)
               ON CONFLICT (branch, name) DO NOTHING"#,
        )
        .bind(branch)
        .bind(name)
        .bind(description)
        .bind(body)
        .bind(enabled)
        .execute(&mut *tx)
        .await?;
        for (path, content) in resources {
            sqlx::query(
                r#"INSERT INTO skill_resources (branch, skill_name, path, content, version)
                   VALUES ($1, $2, $3, $4, 1)
                   ON CONFLICT (branch, skill_name, path) DO NOTHING"#,
            )
            .bind(branch)
            .bind(name)
            .bind(path)
            .bind(content)
            .execute(&mut *tx)
            .await?;
        }
        tx.commit().await?;
        Ok(())
    }
}

fn validate_branch_name(name: &str) -> RuntimeResult<()> {
    let invalid = name.is_empty()
        || name.len() > 64
        || !name
            .chars()
            .next()
            .is_some_and(|first| first.is_ascii_alphanumeric())
        || !name
            .chars()
            .all(|value| value.is_ascii_alphanumeric() || matches!(value, '-' | '_' | '.'));
    if invalid {
        return Err(RuntimeError::InvalidRequest(
            "branch names must be 1-64 characters, start alphanumeric, and contain only letters, digits, '-', '_' or '.'"
                .into(),
        ));
    }
    Ok(())
}

fn row_to_branch(row: &sqlx::postgres::PgRow) -> RuntimeResult<KbBranchRecord> {
    let status_raw: String = row.try_get("status")?;
    let status = KbBranchStatus::parse(&status_raw)
        .ok_or_else(|| RuntimeError::Internal(format!("invalid branch status `{status_raw}`")))?;
    let base_version: i64 = row.try_get("base_version")?;
    let head_version: i64 = row.try_get("head_version")?;
    Ok(KbBranchRecord {
        name: row.try_get("name")?,
        description: row.try_get("description")?,
        status,
        base_version: u64::try_from(base_version).unwrap_or(0),
        head_version: u64::try_from(head_version).unwrap_or(0),
        created_at: row.try_get("created_at")?,
        updated_at: row.try_get("updated_at")?,
    })
}

fn row_to_document(row: &sqlx::postgres::PgRow) -> RuntimeResult<KbDocumentRecord> {
    let version: i64 = row.try_get("version")?;
    Ok(KbDocumentRecord {
        doc_id: row.try_get("doc_id")?,
        name: row.try_get("name")?,
        content: row.try_get("content")?,
        version: u64::try_from(version).unwrap_or(0),
        updated_at: row.try_get("updated_at")?,
    })
}

fn row_to_agent(row: &sqlx::postgres::PgRow) -> RuntimeResult<AgentDefinitionRecord> {
    let tools: Value = row.try_get("allowed_tools")?;
    let version: i64 = row.try_get("version")?;
    Ok(AgentDefinitionRecord {
        name: row.try_get("name")?,
        model: row.try_get("model")?,
        system_prompt: row.try_get("system_prompt")?,
        allowed_tools: serde_json::from_value(tools).unwrap_or_default(),
        version: u64::try_from(version).unwrap_or(0),
        enabled: row.try_get("enabled")?,
        updated_at: row.try_get("updated_at")?,
    })
}

async fn fetch_branch_for_update(
    conn: &mut PgConnection,
    name: &str,
) -> RuntimeResult<Option<KbBranchRecord>> {
    let row = sqlx::query(
        r#"SELECT name, description, status, base_version, head_version, created_at, updated_at
           FROM kb_branches WHERE name = $1 FOR UPDATE"#,
    )
    .bind(name)
    .fetch_optional(&mut *conn)
    .await?;
    row.as_ref().map(row_to_branch).transpose()
}

async fn fetch_active_branch_for_update(conn: &mut PgConnection) -> RuntimeResult<KbBranchRecord> {
    let row = sqlx::query(
        r#"SELECT name, description, status, base_version, head_version, created_at, updated_at
           FROM kb_branches WHERE status = 'active' ORDER BY name ASC LIMIT 1 FOR UPDATE"#,
    )
    .fetch_optional(&mut *conn)
    .await?
    .ok_or_else(|| RuntimeError::Internal("no active knowledge-base branch".into()))?;
    row_to_branch(&row)
}

async fn fetch_conflict_mark(
    conn: &mut PgConnection,
    branch: &str,
) -> RuntimeResult<Option<KbConflictMark>> {
    let row = sqlx::query(
        "SELECT branch_head_version FROM kb_branch_conflicts WHERE branch = $1 FOR UPDATE",
    )
    .bind(branch)
    .fetch_optional(&mut *conn)
    .await?;
    row.map(|row| {
        let branch_head_version: i64 = row.try_get("branch_head_version")?;
        Ok(KbConflictMark {
            branch_head_version: u64::try_from(branch_head_version).unwrap_or(0),
        })
    })
    .transpose()
}

async fn upsert_conflict_mark(
    conn: &mut PgConnection,
    branch: &str,
    active_version: u64,
    branch_head_version: u64,
) -> RuntimeResult<()> {
    sqlx::query(
        r#"INSERT INTO kb_branch_conflicts (branch, active_version, branch_head_version)
           VALUES ($1, $2, $3)
           ON CONFLICT (branch) DO UPDATE
           SET active_version = EXCLUDED.active_version,
               branch_head_version = EXCLUDED.branch_head_version,
               created_at = NOW()"#,
    )
    .bind(branch)
    .bind(i64::try_from(active_version).unwrap_or(i64::MAX))
    .bind(i64::try_from(branch_head_version).unwrap_or(i64::MAX))
    .execute(&mut *conn)
    .await?;
    Ok(())
}

async fn delete_conflict_mark(conn: &mut PgConnection, branch: &str) -> RuntimeResult<()> {
    sqlx::query("DELETE FROM kb_branch_conflicts WHERE branch = $1")
        .bind(branch)
        .execute(&mut *conn)
        .await?;
    Ok(())
}

async fn fetch_entries(conn: &mut PgConnection, branch: &str) -> RuntimeResult<KbEntries> {
    let document_rows = sqlx::query(
        r#"SELECT doc_id, name, content, version, updated_at FROM kb_documents
           WHERE branch = $1 ORDER BY doc_id ASC"#,
    )
    .bind(branch)
    .fetch_all(&mut *conn)
    .await?;
    let mut documents = Vec::new();
    for row in &document_rows {
        documents.push(row_to_document(row)?);
    }
    let agent_rows = sqlx::query(
        r#"SELECT name, model, system_prompt, allowed_tools, version, enabled, updated_at
           FROM agent_definitions WHERE branch = $1 ORDER BY name ASC"#,
    )
    .bind(branch)
    .fetch_all(&mut *conn)
    .await?;
    let mut agents = Vec::new();
    for row in &agent_rows {
        agents.push(row_to_agent(row)?);
    }
    let skill_rows = sqlx::query(
        r#"SELECT name, description, body, version, enabled, updated_at
           FROM skill_definitions WHERE branch = $1 ORDER BY name ASC"#,
    )
    .bind(branch)
    .fetch_all(&mut *conn)
    .await?;
    let mut skills = Vec::new();
    for row in &skill_rows {
        let name: String = row.try_get("name")?;
        let version: i64 = row.try_get("version")?;
        let resource_rows = sqlx::query(
            r#"SELECT path, content, version FROM skill_resources
               WHERE branch = $1 AND skill_name = $2 ORDER BY path ASC"#,
        )
        .bind(branch)
        .bind(&name)
        .fetch_all(&mut *conn)
        .await?;
        let mut resources = Vec::new();
        for resource in &resource_rows {
            let resource_version: i64 = resource.try_get("version")?;
            resources.push(SkillResourceRecord {
                path: resource.try_get("path")?,
                content: resource.try_get("content")?,
                version: u64::try_from(resource_version).unwrap_or(0),
            });
        }
        skills.push(SkillDefinitionRecord {
            name,
            description: row.try_get("description")?,
            body: row.try_get("body")?,
            version: u64::try_from(version).unwrap_or(0),
            enabled: row.try_get("enabled")?,
            resources,
            updated_at: row.try_get("updated_at")?,
        });
    }
    Ok(KbEntries {
        documents,
        agents,
        skills,
    })
}

async fn copy_branch_entries(
    conn: &mut PgConnection,
    source: &str,
    target: &str,
) -> RuntimeResult<()> {
    sqlx::query(
        r#"INSERT INTO kb_documents (branch, doc_id, name, content, version, updated_at)
           SELECT $1, doc_id, name, content, version, updated_at
           FROM kb_documents WHERE branch = $2"#,
    )
    .bind(target)
    .bind(source)
    .execute(&mut *conn)
    .await?;
    sqlx::query(
        r#"INSERT INTO agent_definitions
           (branch, name, model, system_prompt, allowed_tools, version, enabled, updated_at)
           SELECT $1, name, model, system_prompt, allowed_tools, version, enabled, updated_at
           FROM agent_definitions WHERE branch = $2"#,
    )
    .bind(target)
    .bind(source)
    .execute(&mut *conn)
    .await?;
    sqlx::query(
        r#"INSERT INTO skill_definitions
           (branch, name, description, body, version, enabled, updated_at)
           SELECT $1, name, description, body, version, enabled, updated_at
           FROM skill_definitions WHERE branch = $2"#,
    )
    .bind(target)
    .bind(source)
    .execute(&mut *conn)
    .await?;
    sqlx::query(
        r#"INSERT INTO skill_resources (branch, skill_name, path, content, version)
           SELECT $1, skill_name, path, content, version
           FROM skill_resources WHERE branch = $2"#,
    )
    .bind(target)
    .bind(source)
    .execute(&mut *conn)
    .await?;
    Ok(())
}

async fn insert_snapshot(
    conn: &mut PgConnection,
    version: u64,
    branch: &str,
    source: &str,
    entries: &KbEntries,
) -> RuntimeResult<()> {
    let entries = serde_json::to_value(entries)?;
    sqlx::query(
        r#"INSERT INTO kb_snapshots (version, branch, source, entries)
           VALUES ($1, $2, $3, $4)
           ON CONFLICT (version) DO NOTHING"#,
    )
    .bind(i64::try_from(version).unwrap_or(i64::MAX))
    .bind(branch)
    .bind(source)
    .bind(entries)
    .execute(&mut *conn)
    .await?;
    Ok(())
}

async fn load_snapshot_entries(conn: &mut PgConnection, version: u64) -> RuntimeResult<KbEntries> {
    let entries: Value = sqlx::query_scalar("SELECT entries FROM kb_snapshots WHERE version = $1")
        .bind(i64::try_from(version).unwrap_or(i64::MAX))
        .fetch_optional(&mut *conn)
        .await?
        .ok_or_else(|| {
            RuntimeError::InvalidRequest(format!(
                "knowledge-base snapshot version {version} does not exist"
            ))
        })?;
    serde_json::from_value(entries)
        .map_err(|error| RuntimeError::Internal(format!("corrupt snapshot: {error}")))
}

async fn bump_version(conn: &mut PgConnection) -> RuntimeResult<u64> {
    let version: i64 = sqlx::query_scalar(
        "UPDATE kb_meta SET version = version + 1 WHERE singleton RETURNING version",
    )
    .fetch_one(&mut *conn)
    .await?;
    Ok(u64::try_from(version).unwrap_or(0))
}

fn item_map(entries: &KbEntries) -> BTreeMap<(KbItemKind, String), KbItemEntry> {
    let mut items = BTreeMap::new();
    for document in &entries.documents {
        let entry = KbItemEntry::Document(document.clone());
        items.insert((entry.kind(), entry.key().to_string()), entry);
    }
    for agent in &entries.agents {
        let entry = KbItemEntry::Agent(agent.clone());
        items.insert((entry.kind(), entry.key().to_string()), entry);
    }
    for skill in &entries.skills {
        let entry = KbItemEntry::Skill(skill.clone());
        items.insert((entry.kind(), entry.key().to_string()), entry);
    }
    items
}

fn entries_from_map(items: BTreeMap<(KbItemKind, String), KbItemEntry>) -> KbEntries {
    let mut entries = KbEntries::default();
    for entry in items.into_values() {
        match entry {
            KbItemEntry::Document(record) => entries.documents.push(record),
            KbItemEntry::Agent(record) => entries.agents.push(record),
            KbItemEntry::Skill(record) => entries.skills.push(record),
        }
    }
    entries
}

fn merge_entries(
    base: &KbEntries,
    active: &KbEntries,
    branch: &KbEntries,
) -> (KbEntries, Vec<KbConflictEntry>) {
    let base_map = item_map(base);
    let active_map = item_map(active);
    let branch_map = item_map(branch);
    let mut keys = BTreeSet::new();
    keys.extend(base_map.keys().cloned());
    keys.extend(active_map.keys().cloned());
    keys.extend(branch_map.keys().cloned());
    let mut merged = BTreeMap::new();
    let mut conflicts = Vec::new();
    for key in keys {
        let base_entry = base_map.get(&key);
        let active_entry = active_map.get(&key);
        let branch_entry = branch_map.get(&key);
        let base_content = base_entry.map(KbItemEntry::content);
        let active_content = active_entry.map(KbItemEntry::content);
        let branch_content = branch_entry.map(KbItemEntry::content);
        let active_changed = active_content != base_content;
        let branch_changed = branch_content != base_content;
        if branch_changed && active_changed && active_content != branch_content {
            conflicts.push(KbConflictEntry {
                item_kind: key.0,
                item_key: key.1,
                base_content,
                active_content,
                branch_content,
            });
        } else if branch_changed {
            if let Some(entry) = branch_entry {
                merged.insert(key, entry.clone());
            }
        } else if let Some(entry) = active_entry {
            merged.insert(key, entry.clone());
        }
    }
    (entries_from_map(merged), conflicts)
}

async fn apply_entries(
    conn: &mut PgConnection,
    branch: &str,
    desired: &KbEntries,
) -> RuntimeResult<usize> {
    let current = fetch_entries(&mut *conn, branch).await?;
    let current_map = item_map(&current);
    let desired_map = item_map(desired);
    let mut applied = 0;
    for (key, entry) in &desired_map {
        let unchanged = current_map
            .get(key)
            .is_some_and(|current| current.content() == entry.content());
        if !unchanged {
            upsert_item(&mut *conn, branch, entry).await?;
            applied += 1;
        }
    }
    for (key, entry) in &current_map {
        if !desired_map.contains_key(key) {
            delete_item(&mut *conn, branch, entry).await?;
            applied += 1;
        }
    }
    Ok(applied)
}

async fn upsert_item(
    conn: &mut PgConnection,
    branch: &str,
    entry: &KbItemEntry,
) -> RuntimeResult<()> {
    match entry {
        KbItemEntry::Document(record) => {
            sqlx::query(
                r#"INSERT INTO kb_documents (branch, doc_id, name, content, version)
                   VALUES ($1, $2, $3, $4, 1)
                   ON CONFLICT (branch, doc_id) DO UPDATE
                   SET name = EXCLUDED.name, content = EXCLUDED.content,
                       version = kb_documents.version + 1, updated_at = NOW()"#,
            )
            .bind(branch)
            .bind(&record.doc_id)
            .bind(&record.name)
            .bind(&record.content)
            .execute(&mut *conn)
            .await?;
        }
        KbItemEntry::Agent(record) => {
            sqlx::query(
                r#"INSERT INTO agent_definitions
                   (branch, name, model, system_prompt, allowed_tools, version, enabled)
                   VALUES ($1, $2, $3, $4, $5, 1, $6)
                   ON CONFLICT (branch, name) DO UPDATE
                   SET model = EXCLUDED.model, system_prompt = EXCLUDED.system_prompt,
                       allowed_tools = EXCLUDED.allowed_tools, enabled = EXCLUDED.enabled,
                       version = agent_definitions.version + 1, updated_at = NOW()"#,
            )
            .bind(branch)
            .bind(&record.name)
            .bind(&record.model)
            .bind(&record.system_prompt)
            .bind(serde_json::to_value(&record.allowed_tools)?)
            .bind(record.enabled)
            .execute(&mut *conn)
            .await?;
        }
        KbItemEntry::Skill(record) => {
            sqlx::query(
                r#"INSERT INTO skill_definitions (branch, name, description, body, version, enabled)
                   VALUES ($1, $2, $3, $4, 1, $5)
                   ON CONFLICT (branch, name) DO UPDATE
                   SET description = EXCLUDED.description, body = EXCLUDED.body,
                       enabled = EXCLUDED.enabled, version = skill_definitions.version + 1,
                       updated_at = NOW()"#,
            )
            .bind(branch)
            .bind(&record.name)
            .bind(&record.description)
            .bind(&record.body)
            .bind(record.enabled)
            .execute(&mut *conn)
            .await?;
            let version: i64 = sqlx::query_scalar(
                "SELECT version FROM skill_definitions WHERE branch = $1 AND name = $2",
            )
            .bind(branch)
            .bind(&record.name)
            .fetch_one(&mut *conn)
            .await?;
            sqlx::query("DELETE FROM skill_resources WHERE branch = $1 AND skill_name = $2")
                .bind(branch)
                .bind(&record.name)
                .execute(&mut *conn)
                .await?;
            for resource in &record.resources {
                sqlx::query(
                    r#"INSERT INTO skill_resources (branch, skill_name, path, content, version)
                       VALUES ($1, $2, $3, $4, $5)"#,
                )
                .bind(branch)
                .bind(&record.name)
                .bind(&resource.path)
                .bind(&resource.content)
                .bind(version)
                .execute(&mut *conn)
                .await?;
            }
        }
    }
    Ok(())
}

async fn delete_item(
    conn: &mut PgConnection,
    branch: &str,
    entry: &KbItemEntry,
) -> RuntimeResult<()> {
    match entry {
        KbItemEntry::Document(record) => {
            sqlx::query("DELETE FROM kb_documents WHERE branch = $1 AND doc_id = $2")
                .bind(branch)
                .bind(&record.doc_id)
                .execute(&mut *conn)
                .await?;
        }
        KbItemEntry::Agent(record) => {
            sqlx::query("DELETE FROM agent_definitions WHERE branch = $1 AND name = $2")
                .bind(branch)
                .bind(&record.name)
                .execute(&mut *conn)
                .await?;
        }
        KbItemEntry::Skill(record) => {
            sqlx::query("DELETE FROM skill_definitions WHERE branch = $1 AND name = $2")
                .bind(branch)
                .bind(&record.name)
                .execute(&mut *conn)
                .await?;
        }
    }
    Ok(())
}

fn parse_frontmatter(raw: &str) -> RuntimeResult<(Option<Frontmatter>, String)> {
    let trimmed = raw.trim_start_matches('\u{feff}').trim_start();
    let Some(rest) = trimmed.strip_prefix("---") else {
        return Ok((None, raw.trim().to_string()));
    };
    let Some((yaml, body)) = rest.split_once("\n---") else {
        return Ok((None, raw.trim().to_string()));
    };
    let frontmatter: Frontmatter = serde_yaml_ng::from_str(yaml.trim()).map_err(|error| {
        RuntimeError::InvalidRequest(format!("invalid YAML frontmatter: {error}"))
    })?;
    Ok((Some(frontmatter), body.trim().to_string()))
}

fn is_markdown(path: &Path) -> bool {
    path.extension()
        .and_then(|extension| extension.to_str())
        .is_some_and(|extension| extension.eq_ignore_ascii_case("md"))
}

fn read_skill_resources(directory: &Path) -> RuntimeResult<Vec<(String, String)>> {
    let mut resources = BTreeMap::new();
    collect_skill_files(directory, directory, &mut resources, 0)?;
    Ok(resources.into_iter().collect())
}

fn collect_skill_files(
    root: &Path,
    current: &Path,
    resources: &mut BTreeMap<String, String>,
    depth: usize,
) -> RuntimeResult<()> {
    if depth > 8 {
        return Ok(());
    }
    for entry in std::fs::read_dir(current)? {
        let entry = entry?;
        let path = entry.path();
        let Ok(relative) = path.strip_prefix(root) else {
            continue;
        };
        let relative = relative.to_string_lossy().replace('\\', "/");
        if relative == "SKILL.md" {
            continue;
        }
        if path.is_dir() {
            collect_skill_files(root, &path, resources, depth + 1)?;
        } else if let Ok(bytes) = std::fs::read(&path)
            && let Ok(text) = String::from_utf8(bytes)
        {
            resources.insert(relative, text);
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn document(doc_id: &str, content: &str) -> KbDocumentRecord {
        KbDocumentRecord {
            doc_id: doc_id.to_string(),
            name: doc_id.to_string(),
            content: content.to_string(),
            version: 1,
            updated_at: Utc::now(),
        }
    }

    #[test]
    fn frontmatter_and_body_are_split() {
        let raw = "---\nname: reviewer\nmodel: gpt-test\nallowed_tools: [lint_document, read_blocks]\n---\n\nReview carefully.\n";
        let (frontmatter, body) = parse_frontmatter(raw).unwrap();
        let frontmatter = frontmatter.expect("frontmatter");
        assert_eq!(frontmatter.name.as_deref(), Some("reviewer"));
        assert_eq!(
            frontmatter.allowed_tools,
            vec!["lint_document", "read_blocks"]
        );
        assert_eq!(body, "Review carefully.");
    }

    #[test]
    fn missing_frontmatter_keeps_the_whole_document_as_body() {
        let (frontmatter, body) = parse_frontmatter("Plain markdown").unwrap();
        assert!(frontmatter.is_none());
        assert_eq!(body, "Plain markdown");
    }

    #[test]
    fn disjoint_item_changes_auto_merge() {
        let base = KbEntries {
            documents: vec![document("shared", "base")],
            ..KbEntries::default()
        };
        let active = KbEntries {
            documents: vec![document("shared", "base"), document("active-only", "A")],
            ..KbEntries::default()
        };
        let branch = KbEntries {
            documents: vec![document("shared", "base"), document("branch-only", "B")],
            ..KbEntries::default()
        };
        let (merged, conflicts) = merge_entries(&base, &active, &branch);
        assert!(conflicts.is_empty());
        let keys: Vec<&str> = merged
            .documents
            .iter()
            .map(|document| document.doc_id.as_str())
            .collect();
        assert_eq!(keys, vec!["active-only", "branch-only", "shared"]);
    }

    #[test]
    fn overlapping_item_changes_report_three_way_content() {
        let base = KbEntries {
            documents: vec![document("shared", "base")],
            agents: Vec::new(),
            skills: Vec::new(),
        };
        let active = KbEntries {
            documents: vec![document("shared", "active")],
            agents: Vec::new(),
            skills: Vec::new(),
        };
        let branch = KbEntries {
            documents: vec![document("shared", "branch")],
            agents: Vec::new(),
            skills: Vec::new(),
        };
        let (_merged, conflicts) = merge_entries(&base, &active, &branch);
        assert_eq!(conflicts.len(), 1);
        let conflict = conflicts.first().expect("conflict");
        assert_eq!(conflict.item_kind, KbItemKind::Doc);
        assert_eq!(conflict.item_key, "shared");
        assert_eq!(
            conflict
                .base_content
                .as_ref()
                .and_then(|value| value.get("content"))
                .and_then(Value::as_str),
            Some("base")
        );
        assert_eq!(
            conflict
                .active_content
                .as_ref()
                .and_then(|value| value.get("content"))
                .and_then(Value::as_str),
            Some("active")
        );
        assert_eq!(
            conflict
                .branch_content
                .as_ref()
                .and_then(|value| value.get("content"))
                .and_then(Value::as_str),
            Some("branch")
        );
    }

    #[test]
    fn branch_names_are_restricted_to_safe_tokens() {
        assert!(validate_branch_name("draft-2026.11_a").is_ok());
        assert!(validate_branch_name("").is_err());
        assert!(validate_branch_name("-leading").is_err());
        assert!(validate_branch_name("has space").is_err());
        assert!(validate_branch_name("slash/name").is_err());
    }
}
