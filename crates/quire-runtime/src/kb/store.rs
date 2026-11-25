use super::*;

impl KbService {
    pub(crate) async fn resolve_branch(
        &self,
        branch: Option<&str>,
    ) -> RuntimeResult<KbBranchRecord> {
        match branch {
            Some(name) => self.require_branch(name.trim()).await,
            None => self.active_branch().await,
        }
    }

    pub(crate) async fn require_branch(&self, name: &str) -> RuntimeResult<KbBranchRecord> {
        if name.is_empty() {
            return Err(RuntimeError::InvalidRequest(
                "branch name must not be empty".into(),
            ));
        }
        self.get_branch(name)
            .await?
            .ok_or_else(|| RuntimeError::InvalidRequest(format!("branch `{name}` does not exist")))
    }

    pub(crate) async fn mutable_branch(&self, name: &str) -> RuntimeResult<KbBranchRecord> {
        let branch = self.require_branch(name).await?;
        if branch.status == KbBranchStatus::Archived {
            return Err(RuntimeError::InvalidRequest(format!(
                "branch `{name}` is archived and read-only"
            )));
        }
        Ok(branch)
    }

    pub(crate) async fn after_kb_mutation_on(
        &self,
        branch: &str,
        status: KbBranchStatus,
    ) -> RuntimeResult<u64> {
        let version = match status {
            KbBranchStatus::Active => {
                let version = self.store.bump_kb_version().await?;
                sqlx::query(
                    r#"UPDATE kb.branches SET head_version = $2, updated_at = NOW()
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
                    r#"UPDATE kb.branches SET head_version = head_version + 1, updated_at = NOW()
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

    pub(crate) async fn after_active_commit(&self, version: u64) {
        self.invalidate_prompt_cache().await;
        if let Err(error) = self.store.bus().publish_kb_invalidation(version).await {
            warn!(%error, version, "failed to publish KB invalidation after branch release");
        }
    }

    pub(crate) async fn invalidate_prompt_cache(&self) {
        self.prompt_cache.invalidate_all();
        self.prompt_cache.run_pending_tasks().await;
    }
}

pub(crate) fn validate_branch_name(name: &str) -> RuntimeResult<()> {
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

pub(crate) fn row_to_branch(row: &sqlx::postgres::PgRow) -> RuntimeResult<KbBranchRecord> {
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

pub(crate) fn row_to_document(row: &sqlx::postgres::PgRow) -> RuntimeResult<KbDocumentRecord> {
    let version: i64 = row.try_get("version")?;
    Ok(KbDocumentRecord {
        doc_id: row.try_get("doc_id")?,
        name: row.try_get("name")?,
        content: row.try_get("content")?,
        version: u64::try_from(version).unwrap_or(0),
        updated_at: row.try_get("updated_at")?,
    })
}

pub(crate) fn row_to_agent(row: &sqlx::postgres::PgRow) -> RuntimeResult<AgentDefinitionRecord> {
    let tools: Value = row.try_get("allowed_tools")?;
    let version: i64 = row.try_get("version")?;
    Ok(AgentDefinitionRecord {
        name: row.try_get("name")?,
        description: row.try_get("description").unwrap_or_default(),
        model: row.try_get("model")?,
        system_prompt: row.try_get("system_prompt")?,
        allowed_tools: serde_json::from_value(tools).unwrap_or_default(),
        is_default: row.try_get("is_default").unwrap_or(false),
        version: u64::try_from(version).unwrap_or(0),
        enabled: row.try_get("enabled")?,
        updated_at: row.try_get("updated_at")?,
    })
}

pub(crate) async fn fetch_branch_for_update(
    conn: &mut PgConnection,
    name: &str,
) -> RuntimeResult<Option<KbBranchRecord>> {
    let row = sqlx::query(
        r#"SELECT name, description, status, base_version, head_version, created_at, updated_at
           FROM kb.branches WHERE name = $1 FOR UPDATE"#,
    )
    .bind(name)
    .fetch_optional(&mut *conn)
    .await?;
    row.as_ref().map(row_to_branch).transpose()
}

pub(crate) async fn fetch_active_branch_for_update(
    conn: &mut PgConnection,
) -> RuntimeResult<KbBranchRecord> {
    let row = sqlx::query(
        r#"SELECT name, description, status, base_version, head_version, created_at, updated_at
           FROM kb.branches WHERE status = 'active' ORDER BY name ASC LIMIT 1 FOR UPDATE"#,
    )
    .fetch_optional(&mut *conn)
    .await?
    .ok_or_else(|| RuntimeError::Internal("no active knowledge-base branch".into()))?;
    row_to_branch(&row)
}

pub(crate) async fn fetch_conflict_mark(
    conn: &mut PgConnection,
    branch: &str,
) -> RuntimeResult<Option<KbConflictMark>> {
    let row = sqlx::query(
        "SELECT branch_head_version FROM kb.branch_conflicts WHERE branch = $1 FOR UPDATE",
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

pub(crate) async fn upsert_conflict_mark(
    conn: &mut PgConnection,
    branch: &str,
    active_version: u64,
    branch_head_version: u64,
) -> RuntimeResult<()> {
    sqlx::query(
        r#"INSERT INTO kb.branch_conflicts (branch, active_version, branch_head_version)
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

pub(crate) async fn delete_conflict_mark(
    conn: &mut PgConnection,
    branch: &str,
) -> RuntimeResult<()> {
    sqlx::query("DELETE FROM kb.branch_conflicts WHERE branch = $1")
        .bind(branch)
        .execute(&mut *conn)
        .await?;
    Ok(())
}

pub(crate) async fn fetch_entries(
    conn: &mut PgConnection,
    branch: &str,
) -> RuntimeResult<KbEntries> {
    let document_rows = sqlx::query(
        r#"SELECT doc_id, name, content, version, updated_at FROM kb.documents
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
        r#"SELECT name, description, model, system_prompt, allowed_tools, is_default, version, enabled, updated_at
           FROM kb.agent_definitions WHERE branch = $1 ORDER BY name ASC"#,
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
           FROM kb.skill_definitions WHERE branch = $1 ORDER BY name ASC"#,
    )
    .bind(branch)
    .fetch_all(&mut *conn)
    .await?;
    let mut skills = Vec::new();
    for row in &skill_rows {
        let name: String = row.try_get("name")?;
        let version: i64 = row.try_get("version")?;
        let resource_rows = sqlx::query(
            r#"SELECT path, content, version FROM kb.skill_resources
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

pub(crate) async fn copy_branch_entries(
    conn: &mut PgConnection,
    source: &str,
    target: &str,
) -> RuntimeResult<()> {
    sqlx::query(
        r#"INSERT INTO kb.documents (branch, doc_id, name, content, version, updated_at)
           SELECT $1, doc_id, name, content, version, updated_at
           FROM kb.documents WHERE branch = $2"#,
    )
    .bind(target)
    .bind(source)
    .execute(&mut *conn)
    .await?;
    sqlx::query(
        r#"INSERT INTO kb.agent_definitions
           (branch, name, description, model, system_prompt, allowed_tools, is_default, version, enabled, updated_at)
           SELECT $1, name, description, model, system_prompt, allowed_tools, is_default, version, enabled, updated_at
           FROM kb.agent_definitions WHERE branch = $2"#,
    )
    .bind(target)
    .bind(source)
    .execute(&mut *conn)
    .await?;
    sqlx::query(
        r#"INSERT INTO kb.skill_definitions
           (branch, name, description, body, version, enabled, updated_at)
           SELECT $1, name, description, body, version, enabled, updated_at
           FROM kb.skill_definitions WHERE branch = $2"#,
    )
    .bind(target)
    .bind(source)
    .execute(&mut *conn)
    .await?;
    sqlx::query(
        r#"INSERT INTO kb.skill_resources (branch, skill_name, path, content, version)
           SELECT $1, skill_name, path, content, version
           FROM kb.skill_resources WHERE branch = $2"#,
    )
    .bind(target)
    .bind(source)
    .execute(&mut *conn)
    .await?;
    Ok(())
}

pub(crate) async fn insert_snapshot(
    conn: &mut PgConnection,
    version: u64,
    branch: &str,
    source: &str,
    entries: &KbEntries,
) -> RuntimeResult<()> {
    let entries = serde_json::to_value(entries)?;
    sqlx::query(
        r#"INSERT INTO kb.snapshots (version, branch, source, entries)
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

pub(crate) async fn load_snapshot_entries(
    conn: &mut PgConnection,
    version: u64,
) -> RuntimeResult<KbEntries> {
    let entries: Value = sqlx::query_scalar("SELECT entries FROM kb.snapshots WHERE version = $1")
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

pub(crate) async fn bump_version(conn: &mut PgConnection) -> RuntimeResult<u64> {
    let version: i64 = sqlx::query_scalar(
        "UPDATE kb.meta SET version = version + 1 WHERE singleton RETURNING version",
    )
    .fetch_one(&mut *conn)
    .await?;
    Ok(u64::try_from(version).unwrap_or(0))
}

pub(crate) fn item_map(entries: &KbEntries) -> BTreeMap<(KbItemKind, String), KbItemEntry> {
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

pub(crate) fn entries_from_map(items: BTreeMap<(KbItemKind, String), KbItemEntry>) -> KbEntries {
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

pub(crate) fn merge_entries(
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

pub(crate) async fn apply_entries(
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

pub(crate) async fn upsert_item(
    conn: &mut PgConnection,
    branch: &str,
    entry: &KbItemEntry,
) -> RuntimeResult<()> {
    match entry {
        KbItemEntry::Document(record) => {
            sqlx::query(
                r#"INSERT INTO kb.documents (branch, doc_id, name, content, version)
                   VALUES ($1, $2, $3, $4, 1)
                   ON CONFLICT (branch, doc_id) DO UPDATE
                   SET name = EXCLUDED.name, content = EXCLUDED.content,
                       version = kb.documents.version + 1, updated_at = NOW()"#,
            )
            .bind(branch)
            .bind(&record.doc_id)
            .bind(&record.name)
            .bind(&record.content)
            .execute(&mut *conn)
            .await?;
        }
        KbItemEntry::Agent(record) => {
            if record.is_default {
                sqlx::query(
                    r#"UPDATE kb.agent_definitions
                       SET is_default = FALSE, updated_at = NOW()
                       WHERE branch = $1 AND name <> $2 AND is_default = TRUE"#,
                )
                .bind(branch)
                .bind(&record.name)
                .execute(&mut *conn)
                .await?;
            }
            sqlx::query(
                r#"INSERT INTO kb.agent_definitions
                   (branch, name, description, model, system_prompt, allowed_tools, is_default, version, enabled)
                   VALUES ($1, $2, $3, $4, $5, $6, $7, 1, $8)
                   ON CONFLICT (branch, name) DO UPDATE
                   SET description = EXCLUDED.description, model = EXCLUDED.model,
                       system_prompt = EXCLUDED.system_prompt, allowed_tools = EXCLUDED.allowed_tools,
                       is_default = EXCLUDED.is_default, enabled = EXCLUDED.enabled,
                       version = kb.agent_definitions.version + 1, updated_at = NOW()"#,
            )
            .bind(branch)
            .bind(&record.name)
            .bind(&record.description)
            .bind(&record.model)
            .bind(&record.system_prompt)
            .bind(serde_json::to_value(&record.allowed_tools)?)
            .bind(record.is_default)
            .bind(record.enabled)
            .execute(&mut *conn)
            .await?;
        }
        KbItemEntry::Skill(record) => {
            sqlx::query(
                r#"INSERT INTO kb.skill_definitions (branch, name, description, body, version, enabled)
                   VALUES ($1, $2, $3, $4, 1, $5)
                   ON CONFLICT (branch, name) DO UPDATE
                   SET description = EXCLUDED.description, body = EXCLUDED.body,
                       enabled = EXCLUDED.enabled, version = kb.skill_definitions.version + 1,
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
                "SELECT version FROM kb.skill_definitions WHERE branch = $1 AND name = $2",
            )
            .bind(branch)
            .bind(&record.name)
            .fetch_one(&mut *conn)
            .await?;
            sqlx::query("DELETE FROM kb.skill_resources WHERE branch = $1 AND skill_name = $2")
                .bind(branch)
                .bind(&record.name)
                .execute(&mut *conn)
                .await?;
            for resource in &record.resources {
                sqlx::query(
                    r#"INSERT INTO kb.skill_resources (branch, skill_name, path, content, version)
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

pub(crate) async fn delete_item(
    conn: &mut PgConnection,
    branch: &str,
    entry: &KbItemEntry,
) -> RuntimeResult<()> {
    match entry {
        KbItemEntry::Document(record) => {
            sqlx::query("DELETE FROM kb.documents WHERE branch = $1 AND doc_id = $2")
                .bind(branch)
                .bind(&record.doc_id)
                .execute(&mut *conn)
                .await?;
        }
        KbItemEntry::Agent(record) => {
            sqlx::query("DELETE FROM kb.agent_definitions WHERE branch = $1 AND name = $2")
                .bind(branch)
                .bind(&record.name)
                .execute(&mut *conn)
                .await?;
        }
        KbItemEntry::Skill(record) => {
            sqlx::query("DELETE FROM kb.skill_definitions WHERE branch = $1 AND name = $2")
                .bind(branch)
                .bind(&record.name)
                .execute(&mut *conn)
                .await?;
        }
    }
    Ok(())
}
