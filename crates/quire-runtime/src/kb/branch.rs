use super::*;

impl KbService {
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
        let default_agent = self.get_default_agent_on(Some(&branch)).await?;
        let docs = self.list_documents_on(Some(&branch)).await?;
        let skills = self.list_skills_on(Some(&branch)).await?;
        let mut prompt = default_agent
            .map(|agent| agent.system_prompt)
            .filter(|value| !value.trim().is_empty())
            .unwrap_or_else(|| {
                "You are the Quire report agent. Use the provided MCP tools to author, review and lint reports."
                    .to_string()
            });
        for doc in &docs {
            if !doc.content.trim().is_empty() {
                prompt.push_str("\n\n");
                prompt.push_str(&doc.content);
            }
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
        crate::bus::spawn_resilient_invalidation_listener(
            self.store.bus().clone(),
            crate::bus::RedisBus::kb_invalidation_subscription,
            Box::new(move || {
                let cache = cache.clone();
                Box::pin(async move {
                    cache.invalidate_all();
                    cache.run_pending_tasks().await;
                })
            }),
        )
    }
}
