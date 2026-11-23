//! Global MCP server registry, tool discovery and tool-level enable switches.

use std::{
    collections::{BTreeMap, BTreeSet, HashMap},
    sync::Arc,
};

use chrono::{DateTime, Utc};
use moka::future::Cache;
use schemars::JsonSchema;
use secrecy::SecretString;
use serde::{Deserialize, Serialize};
use serde_json::Value;
use sqlx::Row;
use tracing::warn;

use crate::{
    error::{RuntimeError, RuntimeResult},
    mcp_client::RmcpClient,
    models::{mask_secret_ref, normalize_secret_ref, resolve_api_key_ref},
    store::RuntimeStore,
};

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct McpServerRecord {
    pub server_id: String,
    pub name: String,
    pub url: String,
    /// Never the raw reference: `env:VAR` is retained, encrypted refs are masked.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub auth_token_masked: Option<String>,
    pub enabled: bool,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct McpServerInput {
    pub server_id: String,
    pub name: String,
    pub url: String,
    #[serde(default)]
    pub auth_token_ref: Option<String>,
    #[serde(default = "default_enabled")]
    pub enabled: bool,
}

#[derive(Debug, Clone, Default, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct McpServerPatch {
    #[serde(default)]
    pub name: Option<String>,
    #[serde(default)]
    pub url: Option<String>,
    #[serde(default)]
    pub auth_token_ref: Option<String>,
    #[serde(default)]
    pub enabled: Option<bool>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct McpToolRecord {
    pub server_id: String,
    pub tool_name: String,
    pub description: String,
    pub input_schema: Value,
    pub enabled: bool,
    pub updated_at: DateTime<Utc>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct McpSyncReport {
    pub server_id: String,
    pub discovered: usize,
    pub created: usize,
    pub updated: usize,
    pub tools: Vec<McpToolRecord>,
}

/// One enabled server with the tools the current agent may call. The token stays in memory only.
#[derive(Clone)]
pub struct McpServerBinding {
    pub server_id: String,
    pub name: String,
    pub url: String,
    pub auth_token: Option<SecretString>,
    pub tool_names: Vec<String>,
}

#[derive(Clone)]
pub(crate) struct CachedMcpServer {
    server_id: String,
    name: String,
    url: String,
    auth_token_ref: Option<String>,
    enabled: bool,
}

#[derive(Default)]
pub(crate) struct McpCatalogSnapshot {
    servers: Vec<CachedMcpServer>,
    tools: HashMap<String, HashMap<String, bool>>,
}

#[derive(Clone)]
pub struct McpRegistry {
    store: RuntimeStore,
    master_key: Option<SecretString>,
    cache: Cache<String, Arc<McpCatalogSnapshot>>,
    connections: Cache<String, Arc<RmcpClient>>,
    connection_locks: Cache<String, Arc<tokio::sync::Mutex<()>>>,
}

impl McpRegistry {
    pub fn new(store: RuntimeStore, master_key: Option<SecretString>) -> Self {
        Self {
            store,
            master_key,
            cache: Cache::new(32),
            connections: Cache::new(128),
            connection_locks: Cache::new(128),
        }
    }

    pub fn store(&self) -> &RuntimeStore {
        &self.store
    }

    pub fn spawn_invalidation_listener(&self) -> tokio::task::JoinHandle<()> {
        let cache = self.cache.clone();
        let connections = self.connections.clone();
        crate::bus::spawn_resilient_invalidation_listener(
            self.store.bus().clone(),
            crate::bus::RedisBus::mcp_invalidation_subscription,
            Box::new(move || {
                let cache = cache.clone();
                let connections = connections.clone();
                Box::pin(async move {
                    cache.invalidate_all();
                    cache.run_pending_tasks().await;
                    connections.invalidate_all();
                    connections.run_pending_tasks().await;
                })
            }),
        )
    }

    /// Retire any legacy `quire-report` HTTP MCP server entry now that all 18 Quire
    /// report domain tools run natively in-process.
    pub async fn retire_legacy_quire_report(&self) -> RuntimeResult<()> {
        let deleted = sqlx::query("DELETE FROM mcp_servers WHERE server_id = 'quire-report'")
            .execute(self.store.pool())
            .await?;
        if deleted.rows_affected() > 0 {
            self.after_mutation().await;
        }
        Ok(())
    }

    pub async fn list_servers(&self) -> RuntimeResult<Vec<McpServerRecord>> {
        let rows = sqlx::query(
            r#"SELECT server_id, name, url, auth_token_ref, enabled, created_at, updated_at
               FROM mcp_servers ORDER BY server_id ASC"#,
        )
        .fetch_all(self.store.pool())
        .await?;
        rows.iter().map(row_to_server).collect()
    }

    pub async fn get_server(&self, server_id: &str) -> RuntimeResult<Option<McpServerRecord>> {
        let row = sqlx::query(
            r#"SELECT server_id, name, url, auth_token_ref, enabled, created_at, updated_at
               FROM mcp_servers WHERE server_id = $1"#,
        )
        .bind(server_id)
        .fetch_optional(self.store.pool())
        .await?;
        row.as_ref().map(row_to_server).transpose()
    }

    pub async fn upsert_server(&self, input: McpServerInput) -> RuntimeResult<McpServerRecord> {
        validate_server_id(&input.server_id)?;
        validate_name_url(&input.name, &input.url)?;
        let auth_token_ref = match input.auth_token_ref.as_deref() {
            Some(value) if !value.trim().is_empty() => {
                Some(normalize_secret_ref(value, self.master_key.as_ref())?)
            }
            _ => None,
        };
        sqlx::query(
            r#"INSERT INTO mcp_servers (server_id, name, url, auth_token_ref, enabled)
               VALUES ($1, $2, $3, $4, $5)
               ON CONFLICT (server_id) DO UPDATE
               SET name = EXCLUDED.name,
                   url = EXCLUDED.url,
                   auth_token_ref = COALESCE(EXCLUDED.auth_token_ref, mcp_servers.auth_token_ref),
                   enabled = EXCLUDED.enabled,
                   updated_at = NOW()"#,
        )
        .bind(&input.server_id)
        .bind(&input.name)
        .bind(&input.url)
        .bind(auth_token_ref)
        .bind(input.enabled)
        .execute(self.store.pool())
        .await?;
        self.after_mutation().await;
        self.get_server(&input.server_id)
            .await?
            .ok_or_else(|| RuntimeError::Internal("MCP server vanished".into()))
    }

    pub async fn patch_server(
        &self,
        server_id: &str,
        patch: McpServerPatch,
    ) -> RuntimeResult<McpServerRecord> {
        let current = self.get_server_internal(server_id).await?.ok_or_else(|| {
            RuntimeError::InvalidRequest(format!("MCP server `{server_id}` not found"))
        })?;
        let name = patch.name.unwrap_or(current.name);
        let url = patch.url.unwrap_or(current.url);
        validate_name_url(&name, &url)?;
        let auth_token_ref = match patch.auth_token_ref.as_deref() {
            Some(value) if !value.trim().is_empty() => {
                Some(normalize_secret_ref(value, self.master_key.as_ref())?)
            }
            _ => current.auth_token_ref,
        };
        let enabled = patch.enabled.unwrap_or(current.enabled);
        sqlx::query(
            r#"UPDATE mcp_servers
               SET name = $2, url = $3, auth_token_ref = $4, enabled = $5, updated_at = NOW()
               WHERE server_id = $1"#,
        )
        .bind(server_id)
        .bind(&name)
        .bind(&url)
        .bind(auth_token_ref)
        .bind(enabled)
        .execute(self.store.pool())
        .await?;
        self.after_mutation().await;
        self.get_server(server_id)
            .await?
            .ok_or_else(|| RuntimeError::Internal("MCP server vanished".into()))
    }

    pub async fn delete_server(&self, server_id: &str) -> RuntimeResult<()> {
        sqlx::query("DELETE FROM mcp_servers WHERE server_id = $1")
            .bind(server_id)
            .execute(self.store.pool())
            .await?;
        self.after_mutation().await;
        Ok(())
    }

    pub async fn sync_server_tools(&self, server_id: &str) -> RuntimeResult<McpSyncReport> {
        let server = self.get_server_internal(server_id).await?.ok_or_else(|| {
            RuntimeError::InvalidRequest(format!("MCP server `{server_id}` not found"))
        })?;
        let token = match server.auth_token_ref.as_deref() {
            Some(reference) => Some(resolve_api_key_ref(reference, self.master_key.as_ref())?),
            None => None,
        };
        let client =
            RmcpClient::connect(&server.url, token, "runtime-mcp-registry", "admin").await?;
        let descriptors = client.list_tools().await?;
        let mut discovered = BTreeMap::new();
        for descriptor in descriptors {
            discovered.insert(descriptor.name.clone(), descriptor);
        }
        let existing: BTreeSet<String> =
            sqlx::query_scalar("SELECT tool_name FROM mcp_tools WHERE server_id = $1")
                .bind(server_id)
                .fetch_all(self.store.pool())
                .await?
                .into_iter()
                .collect();
        let mut created = 0_usize;
        let mut updated = 0_usize;
        let mut tx = self.store.pool().begin().await?;
        for (tool_name, descriptor) in &discovered {
            if existing.contains(tool_name) {
                updated += 1;
            } else {
                created += 1;
            }
            sqlx::query(
                r#"INSERT INTO mcp_tools
                   (server_id, tool_name, description, input_schema, enabled)
                   VALUES ($1, $2, $3, $4, TRUE)
                   ON CONFLICT (server_id, tool_name) DO UPDATE
                   SET description = EXCLUDED.description,
                       input_schema = EXCLUDED.input_schema,
                       updated_at = NOW()"#,
            )
            .bind(server_id)
            .bind(tool_name)
            .bind(&descriptor.description)
            .bind(serde_json::to_value(&descriptor.input_schema)?)
            .execute(&mut *tx)
            .await?;
        }
        tx.commit().await?;
        self.after_mutation().await;
        Ok(McpSyncReport {
            server_id: server_id.to_string(),
            discovered: discovered.len(),
            created,
            updated,
            tools: self.list_tools(Some(server_id)).await?,
        })
    }

    pub async fn list_tools(&self, server_id: Option<&str>) -> RuntimeResult<Vec<McpToolRecord>> {
        let rows =
            match server_id {
                Some(server_id) => sqlx::query(
                    r#"SELECT server_id, tool_name, description, input_schema, enabled, updated_at
                       FROM mcp_tools WHERE server_id = $1
                       ORDER BY server_id ASC, tool_name ASC"#,
                )
                .bind(server_id)
                .fetch_all(self.store.pool())
                .await?,
                None => sqlx::query(
                    r#"SELECT server_id, tool_name, description, input_schema, enabled, updated_at
                       FROM mcp_tools
                       ORDER BY server_id ASC, tool_name ASC"#,
                )
                .fetch_all(self.store.pool())
                .await?,
            };
        rows.iter().map(row_to_tool).collect()
    }

    pub async fn set_tool_enabled(
        &self,
        server_id: &str,
        tool_name: &str,
        enabled: bool,
    ) -> RuntimeResult<McpToolRecord> {
        let updated = sqlx::query(
            r#"UPDATE mcp_tools
               SET enabled = $3, updated_at = NOW()
               WHERE server_id = $1 AND tool_name = $2"#,
        )
        .bind(server_id)
        .bind(tool_name)
        .bind(enabled)
        .execute(self.store.pool())
        .await?;
        if updated.rows_affected() == 0 {
            return Err(RuntimeError::InvalidRequest(format!(
                "MCP tool `{server_id}:{tool_name}` not found"
            )));
        }
        self.after_mutation().await;
        self.get_tool(server_id, tool_name)
            .await?
            .ok_or_else(|| RuntimeError::Internal("MCP tool vanished".into()))
    }

    pub async fn get_tool(
        &self,
        server_id: &str,
        tool_name: &str,
    ) -> RuntimeResult<Option<McpToolRecord>> {
        let row = sqlx::query(
            r#"SELECT server_id, tool_name, description, input_schema, enabled, updated_at
               FROM mcp_tools WHERE server_id = $1 AND tool_name = $2"#,
        )
        .bind(server_id)
        .bind(tool_name)
        .fetch_optional(self.store.pool())
        .await?;
        row.as_ref().map(row_to_tool).transpose()
    }

    /// Return a live shared MCP connection, reconnecting only after config invalidation or close.
    pub async fn shared_client(
        &self,
        binding: &McpServerBinding,
    ) -> RuntimeResult<Arc<RmcpClient>> {
        if let Some(client) = self.connections.get(&binding.server_id).await
            && !client.is_closed()
        {
            return Ok(client);
        }
        let lock = self
            .connection_locks
            .get_with(binding.server_id.clone(), async {
                Arc::new(tokio::sync::Mutex::new(()))
            })
            .await;
        let _guard = lock.lock().await;
        if let Some(client) = self.connections.get(&binding.server_id).await
            && !client.is_closed()
        {
            return Ok(client);
        }
        let client =
            Arc::new(RmcpClient::connect_shared(&binding.url, binding.auth_token.clone()).await?);
        self.connections
            .insert(binding.server_id.clone(), Arc::clone(&client))
            .await;
        Ok(client)
    }

    /// Resolve an enabled server for a host-authored operational call using the same shared pool.
    pub async fn shared_client_for_server(
        &self,
        server_id: &str,
    ) -> RuntimeResult<Arc<RmcpClient>> {
        let server = self.get_server_internal(server_id).await?.ok_or_else(|| {
            RuntimeError::InvalidRequest(format!("MCP server `{server_id}` is not registered"))
        })?;
        if !server.enabled {
            return Err(RuntimeError::InvalidRequest(format!(
                "MCP server `{server_id}` is disabled"
            )));
        }
        let auth_token = match server.auth_token_ref.as_deref() {
            Some(reference) => Some(resolve_api_key_ref(reference, self.master_key.as_ref())?),
            None => None,
        };
        self.shared_client(&McpServerBinding {
            server_id: server.server_id,
            name: server.name,
            url: server.url,
            auth_token,
            tool_names: Vec::new(),
        })
        .await
    }

    /// Triple intersection: server enabled, tool globally enabled and agent allow-list matched.
    pub async fn tools_for_agent(
        &self,
        allowed_tools: &[String],
    ) -> RuntimeResult<Vec<McpServerBinding>> {
        let catalog = self.catalog().await?;
        let mut bindings = Vec::new();
        for server in &catalog.servers {
            let mut tool_names: Vec<String> = catalog
                .tools
                .get(&server.server_id)
                .map(|tools| {
                    tools
                        .iter()
                        .filter(|(tool_name, enabled)| {
                            **enabled
                                && agent_allows_mcp_tool(
                                    allowed_tools,
                                    &server.server_id,
                                    tool_name,
                                )
                        })
                        .map(|(tool_name, _)| tool_name.clone())
                        .collect()
                })
                .unwrap_or_default();
            tool_names.sort();
            if tool_names.is_empty() {
                continue;
            }
            let auth_token = match server.auth_token_ref.as_deref() {
                Some(reference) => Some(resolve_api_key_ref(reference, self.master_key.as_ref())?),
                None => None,
            };
            bindings.push(McpServerBinding {
                server_id: server.server_id.clone(),
                name: server.name.clone(),
                url: server.url.clone(),
                auth_token,
                tool_names,
            });
        }
        Ok(bindings)
    }
}

mod catalog;
mod policy;

pub(crate) use catalog::{agent_allows_mcp_tool, tool_pattern_matches};
pub(crate) use policy::*;
