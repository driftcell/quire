use super::*;

impl KbService {
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
               FROM kb.skill_definitions WHERE branch = $1 ORDER BY name ASC"#,
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
            r#"INSERT INTO kb.skill_definitions (branch, name, description, body, version, enabled)
               VALUES ($1, $2, $3, $4, 1, $5)
               ON CONFLICT (branch, name) DO UPDATE
               SET description = EXCLUDED.description, body = EXCLUDED.body,
                   enabled = EXCLUDED.enabled, version = kb.skill_definitions.version + 1,
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
            "SELECT version FROM kb.skill_definitions WHERE branch = $1 AND name = $2",
        )
        .bind(&branch_record.name)
        .bind(name)
        .fetch_one(&mut *tx)
        .await?;
        sqlx::query("DELETE FROM kb.skill_resources WHERE branch = $1 AND skill_name = $2")
            .bind(&branch_record.name)
            .bind(name)
            .execute(&mut *tx)
            .await?;
        for (path, content) in resources {
            sqlx::query(
                r#"INSERT INTO kb.skill_resources (branch, skill_name, path, content, version)
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
        sqlx::query("DELETE FROM kb.skill_definitions WHERE branch = $1 AND name = $2")
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
            r#"SELECT path, content, version FROM kb.skill_resources
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
}
