//! Knowledge-base, agent-definition and skill management with moka caching.

use std::{collections::BTreeMap, path::Path, sync::Arc};

use chrono::{DateTime, Utc};
use moka::future::Cache;
use serde::{Deserialize, Serialize};
use serde_json::Value;
use sqlx::Row;

use crate::{
    error::{RuntimeError, RuntimeResult},
    store::RuntimeStore,
};

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct KbDocumentRecord {
    pub doc_id: String,
    pub name: String,
    pub content: String,
    pub version: u64,
    pub updated_at: DateTime<Utc>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct AgentDefinitionRecord {
    pub name: String,
    pub model: String,
    pub system_prompt: String,
    pub allowed_tools: Vec<String>,
    pub version: u64,
    pub enabled: bool,
    pub updated_at: DateTime<Utc>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct SkillResourceRecord {
    pub path: String,
    pub content: String,
    pub version: u64,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct SkillDefinitionRecord {
    pub name: String,
    pub description: String,
    pub body: String,
    pub version: u64,
    pub enabled: bool,
    pub resources: Vec<SkillResourceRecord>,
    pub updated_at: DateTime<Utc>,
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

    /// Build (and cache) the frozen system prompt from CLAUDE-style docs and skill summaries.
    pub async fn system_prompt(&self) -> RuntimeResult<Arc<String>> {
        if let Some(cached) = self.prompt_cache.get("system_prompt").await {
            return Ok(cached);
        }
        let docs = self.list_documents().await?;
        let skills = self.list_skills().await?;
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
        self.prompt_cache
            .insert("system_prompt".to_string(), prompt.clone())
            .await;
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
        let mut report = SeedReport::default();
        let document_count: i64 = sqlx::query_scalar("SELECT COUNT(*)::bigint FROM kb_documents")
            .fetch_one(self.store.pool())
            .await?;
        if document_count == 0 {
            let claude = workspace.join("CLAUDE.md");
            if claude.is_file() {
                let content = tokio::fs::read_to_string(&claude).await?;
                self.insert_document_raw("claude-md", "CLAUDE.md", &content)
                    .await?;
                report.documents += 1;
            }
        }

        let agent_count: i64 = sqlx::query_scalar("SELECT COUNT(*)::bigint FROM agent_definitions")
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
                    self.insert_agent_raw(&name, &model, &system_prompt, &allowed_tools, enabled)
                        .await?;
                    report.agents += 1;
                }
            }
        }

        let skill_count: i64 = sqlx::query_scalar("SELECT COUNT(*)::bigint FROM skill_definitions")
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
                    self.insert_skill_raw(&name, &description, &body, true, &resources)
                        .await?;
                    report.skills += 1;
                }
            }
        }

        if report.documents + report.agents + report.skills > 0 {
            report.kb_version = self.store.bump_kb_version().await?;
            self.prompt_cache.invalidate_all();
            self.prompt_cache.run_pending_tasks().await;
        } else {
            report.kb_version = self.store.kb_version().await?;
        }
        Ok(report)
    }

    pub async fn list_documents(&self) -> RuntimeResult<Vec<KbDocumentRecord>> {
        let rows = sqlx::query(
            "SELECT doc_id, name, content, version, updated_at FROM kb_documents ORDER BY name ASC",
        )
        .fetch_all(self.store.pool())
        .await?;
        rows.iter()
            .map(|row| {
                let version: i64 = row.try_get("version")?;
                Ok(KbDocumentRecord {
                    doc_id: row.try_get("doc_id")?,
                    name: row.try_get("name")?,
                    content: row.try_get("content")?,
                    version: u64::try_from(version).unwrap_or(0),
                    updated_at: row.try_get("updated_at")?,
                })
            })
            .collect()
    }

    pub async fn upsert_document(
        &self,
        doc_id: &str,
        name: &str,
        content: &str,
    ) -> RuntimeResult<KbDocumentRecord> {
        sqlx::query(
            r#"INSERT INTO kb_documents (doc_id, name, content, version)
               VALUES ($1, $2, $3, 1)
               ON CONFLICT (doc_id) DO UPDATE
               SET name = EXCLUDED.name, content = EXCLUDED.content,
                   version = kb_documents.version + 1, updated_at = NOW()"#,
        )
        .bind(doc_id)
        .bind(name)
        .bind(content)
        .execute(self.store.pool())
        .await?;
        self.after_kb_mutation().await?;
        let row = sqlx::query(
            "SELECT doc_id, name, content, version, updated_at FROM kb_documents WHERE doc_id = $1",
        )
        .bind(doc_id)
        .fetch_one(self.store.pool())
        .await?;
        let version: i64 = row.try_get("version")?;
        Ok(KbDocumentRecord {
            doc_id: row.try_get("doc_id")?,
            name: row.try_get("name")?,
            content: row.try_get("content")?,
            version: u64::try_from(version).unwrap_or(0),
            updated_at: row.try_get("updated_at")?,
        })
    }

    pub async fn delete_document(&self, doc_id: &str) -> RuntimeResult<()> {
        sqlx::query("DELETE FROM kb_documents WHERE doc_id = $1")
            .bind(doc_id)
            .execute(self.store.pool())
            .await?;
        self.after_kb_mutation().await?;
        Ok(())
    }

    pub async fn list_agents(&self) -> RuntimeResult<Vec<AgentDefinitionRecord>> {
        let rows = sqlx::query(
            r#"SELECT name, model, system_prompt, allowed_tools, version, enabled, updated_at
               FROM agent_definitions ORDER BY name ASC"#,
        )
        .fetch_all(self.store.pool())
        .await?;
        rows.iter().map(row_to_agent).collect()
    }

    pub async fn get_agent(&self, name: &str) -> RuntimeResult<Option<AgentDefinitionRecord>> {
        let row = sqlx::query(
            r#"SELECT name, model, system_prompt, allowed_tools, version, enabled, updated_at
               FROM agent_definitions WHERE name = $1"#,
        )
        .bind(name)
        .fetch_optional(self.store.pool())
        .await?;
        row.map(|row| row_to_agent(&row)).transpose()
    }

    pub async fn get_skill(&self, name: &str) -> RuntimeResult<Option<SkillDefinitionRecord>> {
        Ok(self
            .list_skills()
            .await?
            .into_iter()
            .find(|skill| skill.name == name))
    }

    pub async fn upsert_agent(
        &self,
        name: &str,
        model: &str,
        system_prompt: &str,
        allowed_tools: &[String],
        enabled: bool,
    ) -> RuntimeResult<AgentDefinitionRecord> {
        let tools = serde_json::to_value(allowed_tools)?;
        sqlx::query(
            r#"INSERT INTO agent_definitions
               (name, model, system_prompt, allowed_tools, version, enabled)
               VALUES ($1, $2, $3, $4, 1, $5)
               ON CONFLICT (name) DO UPDATE
               SET model = EXCLUDED.model, system_prompt = EXCLUDED.system_prompt,
                   allowed_tools = EXCLUDED.allowed_tools, enabled = EXCLUDED.enabled,
                   version = agent_definitions.version + 1, updated_at = NOW()"#,
        )
        .bind(name)
        .bind(model)
        .bind(system_prompt)
        .bind(tools)
        .bind(enabled)
        .execute(self.store.pool())
        .await?;
        self.after_kb_mutation().await?;
        self.get_agent(name)
            .await?
            .ok_or_else(|| RuntimeError::Internal("agent definition vanished".into()))
    }

    pub async fn delete_agent(&self, name: &str) -> RuntimeResult<()> {
        sqlx::query("DELETE FROM agent_definitions WHERE name = $1")
            .bind(name)
            .execute(self.store.pool())
            .await?;
        self.after_kb_mutation().await?;
        Ok(())
    }

    pub async fn list_skills(&self) -> RuntimeResult<Vec<SkillDefinitionRecord>> {
        let rows = sqlx::query(
            r#"SELECT name, description, body, version, enabled, updated_at
               FROM skill_definitions ORDER BY name ASC"#,
        )
        .fetch_all(self.store.pool())
        .await?;
        let mut skills = Vec::new();
        for row in rows {
            let name: String = row.try_get("name")?;
            let version: i64 = row.try_get("version")?;
            let resources = self.list_skill_resources(&name).await?;
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

    pub async fn upsert_skill(
        &self,
        name: &str,
        description: &str,
        body: &str,
        enabled: bool,
        resources: &[(String, String)],
    ) -> RuntimeResult<SkillDefinitionRecord> {
        let mut tx = self.store.pool().begin().await?;
        sqlx::query(
            r#"INSERT INTO skill_definitions (name, description, body, version, enabled)
               VALUES ($1, $2, $3, 1, $4)
               ON CONFLICT (name) DO UPDATE
               SET description = EXCLUDED.description, body = EXCLUDED.body,
                   enabled = EXCLUDED.enabled, version = skill_definitions.version + 1,
                   updated_at = NOW()"#,
        )
        .bind(name)
        .bind(description)
        .bind(body)
        .bind(enabled)
        .execute(&mut *tx)
        .await?;
        let version: i64 =
            sqlx::query_scalar("SELECT version FROM skill_definitions WHERE name = $1")
                .bind(name)
                .fetch_one(&mut *tx)
                .await?;
        sqlx::query("DELETE FROM skill_resources WHERE skill_name = $1")
            .bind(name)
            .execute(&mut *tx)
            .await?;
        for (path, content) in resources {
            sqlx::query(
                r#"INSERT INTO skill_resources (skill_name, path, content, version)
                   VALUES ($1, $2, $3, $4)"#,
            )
            .bind(name)
            .bind(path)
            .bind(content)
            .bind(version)
            .execute(&mut *tx)
            .await?;
        }
        tx.commit().await?;
        self.after_kb_mutation().await?;
        self.list_skills()
            .await?
            .into_iter()
            .find(|skill| skill.name == name)
            .ok_or_else(|| RuntimeError::Internal("skill definition vanished".into()))
    }

    pub async fn delete_skill(&self, name: &str) -> RuntimeResult<()> {
        sqlx::query("DELETE FROM skill_definitions WHERE name = $1")
            .bind(name)
            .execute(self.store.pool())
            .await?;
        self.after_kb_mutation().await?;
        Ok(())
    }

    async fn list_skill_resources(&self, name: &str) -> RuntimeResult<Vec<SkillResourceRecord>> {
        let rows = sqlx::query(
            "SELECT path, content, version FROM skill_resources WHERE skill_name = $1 ORDER BY path ASC",
        )
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

    async fn after_kb_mutation(&self) -> RuntimeResult<()> {
        let _ = self.store.bump_kb_version().await?;
        self.prompt_cache.invalidate_all();
        self.prompt_cache.run_pending_tasks().await;
        Ok(())
    }

    async fn insert_document_raw(
        &self,
        doc_id: &str,
        name: &str,
        content: &str,
    ) -> RuntimeResult<()> {
        sqlx::query(
            "INSERT INTO kb_documents (doc_id, name, content, version) VALUES ($1, $2, $3, 1) ON CONFLICT (doc_id) DO NOTHING",
        )
        .bind(doc_id)
        .bind(name)
        .bind(content)
        .execute(self.store.pool())
        .await?;
        Ok(())
    }

    async fn insert_agent_raw(
        &self,
        name: &str,
        model: &str,
        system_prompt: &str,
        allowed_tools: &[String],
        enabled: bool,
    ) -> RuntimeResult<()> {
        sqlx::query(
            r#"INSERT INTO agent_definitions
               (name, model, system_prompt, allowed_tools, version, enabled)
               VALUES ($1, $2, $3, $4, 1, $5)
               ON CONFLICT (name) DO NOTHING"#,
        )
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
        name: &str,
        description: &str,
        body: &str,
        enabled: bool,
        resources: &[(String, String)],
    ) -> RuntimeResult<()> {
        let mut tx = self.store.pool().begin().await?;
        sqlx::query(
            "INSERT INTO skill_definitions (name, description, body, version, enabled) VALUES ($1, $2, $3, 1, $4) ON CONFLICT (name) DO NOTHING",
        )
        .bind(name)
        .bind(description)
        .bind(body)
        .bind(enabled)
        .execute(&mut *tx)
        .await?;
        for (path, content) in resources {
            sqlx::query(
                "INSERT INTO skill_resources (skill_name, path, content, version) VALUES ($1, $2, $3, 1) ON CONFLICT (skill_name, path) DO NOTHING",
            )
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
}
