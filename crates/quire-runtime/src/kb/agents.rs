use super::*;

impl KbService {
    pub async fn list_agents(&self) -> RuntimeResult<Vec<AgentDefinitionRecord>> {
        self.list_agents_on(None).await
    }

    pub async fn list_agents_on(
        &self,
        branch: Option<&str>,
    ) -> RuntimeResult<Vec<AgentDefinitionRecord>> {
        let branch = self.resolve_branch(branch).await?.name;
        let rows = sqlx::query(
            r#"SELECT name, description, model, system_prompt, allowed_tools, is_default, version, enabled, updated_at
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
            r#"SELECT name, description, model, system_prompt, allowed_tools, is_default, version, enabled, updated_at
               FROM agent_definitions WHERE branch = $1 AND name = $2"#,
        )
        .bind(&branch)
        .bind(name)
        .fetch_optional(self.store.pool())
        .await?;
        row.as_ref().map(row_to_agent).transpose()
    }

    pub async fn get_default_agent_on(
        &self,
        branch: Option<&str>,
    ) -> RuntimeResult<Option<AgentDefinitionRecord>> {
        let branch = self.resolve_branch(branch).await?.name;
        let row = sqlx::query(
            r#"SELECT name, description, model, system_prompt, allowed_tools, is_default, version, enabled, updated_at
               FROM agent_definitions
               WHERE branch = $1 AND is_default = TRUE AND enabled = TRUE
               LIMIT 1"#,
        )
        .bind(&branch)
        .fetch_optional(self.store.pool())
        .await?;
        row.as_ref().map(row_to_agent).transpose()
    }

    pub async fn upsert_agent(
        &self,
        command: UpsertKbAgentCommand<'_>,
    ) -> RuntimeResult<AgentDefinitionRecord> {
        let branch = self.active_branch().await?.name;
        self.upsert_agent_on(&branch, command).await
    }

    pub async fn upsert_agent_on(
        &self,
        branch: &str,
        command: UpsertKbAgentCommand<'_>,
    ) -> RuntimeResult<AgentDefinitionRecord> {
        let UpsertKbAgentCommand {
            name,
            description,
            model,
            system_prompt,
            allowed_tools,
            is_default,
            enabled,
        } = command;
        let branch_record = self.mutable_branch(branch).await?;
        let tools = serde_json::to_value(allowed_tools)?;
        let mut tx = self.store.pool().begin().await?;
        if is_default {
            sqlx::query(
                r#"UPDATE agent_definitions
                   SET is_default = FALSE, updated_at = NOW()
                   WHERE branch = $1 AND name <> $2 AND is_default = TRUE"#,
            )
            .bind(&branch_record.name)
            .bind(name)
            .execute(&mut *tx)
            .await?;
        }
        sqlx::query(
            r#"INSERT INTO agent_definitions
               (branch, name, description, model, system_prompt, allowed_tools, is_default, version, enabled)
               VALUES ($1, $2, $3, $4, $5, $6, $7, 1, $8)
               ON CONFLICT (branch, name) DO UPDATE
               SET description = EXCLUDED.description, model = EXCLUDED.model,
                   system_prompt = EXCLUDED.system_prompt, allowed_tools = EXCLUDED.allowed_tools,
                   is_default = EXCLUDED.is_default, enabled = EXCLUDED.enabled,
                   version = agent_definitions.version + 1, updated_at = NOW()"#,
        )
        .bind(&branch_record.name)
        .bind(name)
        .bind(description)
        .bind(model)
        .bind(system_prompt)
        .bind(tools)
        .bind(is_default)
        .bind(enabled)
        .execute(&mut *tx)
        .await?;
        tx.commit().await?;
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
}
