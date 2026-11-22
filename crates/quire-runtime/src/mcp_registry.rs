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
struct CachedMcpServer {
    server_id: String,
    name: String,
    url: String,
    auth_token_ref: Option<String>,
    enabled: bool,
}

#[derive(Default)]
struct McpCatalogSnapshot {
    servers: Vec<CachedMcpServer>,
    tools: HashMap<String, HashMap<String, bool>>,
}

#[derive(Clone)]
pub struct McpRegistry {
    store: RuntimeStore,
    master_key: Option<SecretString>,
    cache: Cache<String, Arc<McpCatalogSnapshot>>,
    connections: Cache<String, Arc<RmcpClient>>,
    connection_lock: Arc<tokio::sync::Mutex<()>>,
}

impl McpRegistry {
    pub fn new(store: RuntimeStore, master_key: Option<SecretString>) -> Self {
        Self {
            store,
            master_key,
            cache: Cache::new(32),
            connections: Cache::new(128),
            connection_lock: Arc::new(tokio::sync::Mutex::new(())),
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

    /// Idempotently register the built-in Quire report MCP server. Never overwrites admin state.
    pub async fn seed_default_quire_report(&self, base_url: &str) -> RuntimeResult<()> {
        let inserted = sqlx::query(
            r#"INSERT INTO mcp_servers (server_id, name, url, auth_token_ref, enabled)
               VALUES ('quire-report', 'Quire Report MCP', $1, 'env:QUIRE_RW_TOKEN', TRUE)
               ON CONFLICT (server_id) DO NOTHING"#,
        )
        .bind(base_url)
        .execute(self.store.pool())
        .await?;
        if inserted.rows_affected() > 0 {
            self.after_mutation().await;
        }

        let tool_count: i64 = sqlx::query_scalar(
            "SELECT COUNT(*)::bigint FROM mcp_tools WHERE server_id = 'quire-report'",
        )
        .fetch_one(self.store.pool())
        .await?;
        if (inserted.rows_affected() > 0 || tool_count == 0)
            && let Err(error) = self.sync_server_tools("quire-report").await
        {
            warn!(
                %error,
                "failed to sync default quire-report MCP tools; retry with POST /api/mcp/servers/quire-report/sync"
            );
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
        let _guard = self.connection_lock.lock().await;
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

    async fn get_server_internal(&self, server_id: &str) -> RuntimeResult<Option<CachedMcpServer>> {
        let row = sqlx::query(
            "SELECT server_id, name, url, auth_token_ref, enabled FROM mcp_servers WHERE server_id = $1",
        )
        .bind(server_id)
        .fetch_optional(self.store.pool())
        .await?;
        row.as_ref().map(row_to_cached_server).transpose()
    }
    async fn catalog(&self) -> RuntimeResult<Arc<McpCatalogSnapshot>> {
        if let Some(cached) = self.cache.get("catalog").await {
            return Ok(cached);
        }
        let server_rows = sqlx::query(
            "SELECT server_id, name, url, auth_token_ref, enabled FROM mcp_servers WHERE enabled ORDER BY server_id ASC",
        )
        .fetch_all(self.store.pool())
        .await?;
        let servers = server_rows
            .iter()
            .map(row_to_cached_server)
            .collect::<RuntimeResult<Vec<_>>>()?;
        let tool_rows = sqlx::query("SELECT server_id, tool_name, enabled FROM mcp_tools")
            .fetch_all(self.store.pool())
            .await?;
        let mut tools: HashMap<String, HashMap<String, bool>> = HashMap::new();
        for row in &tool_rows {
            let server_id: String = row.try_get("server_id")?;
            let tool_name: String = row.try_get("tool_name")?;
            let enabled: bool = row.try_get("enabled")?;
            tools
                .entry(server_id)
                .or_default()
                .insert(tool_name, enabled);
        }
        let snapshot = Arc::new(McpCatalogSnapshot { servers, tools });
        self.cache
            .insert("catalog".to_string(), snapshot.clone())
            .await;
        Ok(snapshot)
    }

    async fn after_mutation(&self) {
        self.cache.invalidate_all();
        self.cache.run_pending_tasks().await;
        self.connections.invalidate_all();
        self.connections.run_pending_tasks().await;
        let version = u64::try_from(Utc::now().timestamp_millis()).unwrap_or(0);
        if let Err(error) = self.store.bus().publish_mcp_invalidation(version).await {
            warn!(%error, version, "failed to publish MCP registry invalidation");
        }
    }
}

/// Match one allow-list pattern against a tool name.
///
/// Patterns are trimmed; an empty pattern never matches. A pattern without `*`
/// is an exact match. A pattern with `*` supports prefix (`subjectcloud_*`),
/// suffix (`*_detail`) and multi-segment wildcards. Scoped MCP patterns are
/// matched by the caller against `server_id:tool_name`.
pub(crate) fn tool_pattern_matches(pattern: &str, candidate: &str) -> bool {
    let pattern = pattern.trim();
    if pattern.is_empty() {
        return false;
    }
    if pattern == "*" {
        return true;
    }
    if !pattern.contains('*') {
        return pattern == candidate;
    }

    let mut segments = pattern.split('*').peekable();
    let Some(first) = segments.next() else {
        return false;
    };
    let Some(mut remaining) = candidate.strip_prefix(first) else {
        return false;
    };
    while let Some(segment) = segments.next() {
        if segments.peek().is_none() {
            return remaining.ends_with(segment);
        }
        if segment.is_empty() {
            continue;
        }
        let Some(index) = remaining.find(segment) else {
            return false;
        };
        let Some(rest) = remaining.get(index.saturating_add(segment.len())..) else {
            return false;
        };
        remaining = rest;
    }
    true
}

/// Match an agent allow-list entry against a server-scoped tool.
pub fn agent_allows_mcp_tool(allowed_tools: &[String], server_id: &str, tool_name: &str) -> bool {
    let scoped = format!("{server_id}:{tool_name}");
    allowed_tools.iter().any(|pattern| {
        let pattern = pattern.trim();
        if pattern.contains(':') {
            tool_pattern_matches(pattern, &scoped)
        } else {
            tool_pattern_matches(pattern, tool_name)
        }
    })
}

fn default_enabled() -> bool {
    true
}

fn validate_server_id(server_id: &str) -> RuntimeResult<()> {
    let invalid = server_id.is_empty()
        || server_id.len() > 64
        || !server_id
            .chars()
            .next()
            .is_some_and(|first| first.is_ascii_alphanumeric())
        || !server_id
            .chars()
            .all(|value| value.is_ascii_alphanumeric() || matches!(value, '-' | '_' | '.'));
    if invalid {
        return Err(RuntimeError::InvalidRequest(
            "MCP server_id must be 1-64 characters, start alphanumeric, and contain only letters, digits, '-', '_' or '.'"
                .into(),
        ));
    }
    Ok(())
}

fn validate_name_url(name: &str, url: &str) -> RuntimeResult<()> {
    if name.trim().is_empty() {
        return Err(RuntimeError::InvalidRequest(
            "MCP server name must not be empty".into(),
        ));
    }
    let url = url.trim();
    if !(url.starts_with("http://") || url.starts_with("https://")) {
        return Err(RuntimeError::InvalidRequest(
            "MCP server url must start with http:// or https://".into(),
        ));
    }
    Ok(())
}

fn row_to_server(row: &sqlx::postgres::PgRow) -> RuntimeResult<McpServerRecord> {
    let auth_token_ref: Option<String> = row.try_get("auth_token_ref")?;
    Ok(McpServerRecord {
        server_id: row.try_get("server_id")?,
        name: row.try_get("name")?,
        url: row.try_get("url")?,
        auth_token_masked: auth_token_ref.as_deref().map(mask_secret_ref),
        enabled: row.try_get("enabled")?,
        created_at: row.try_get("created_at")?,
        updated_at: row.try_get("updated_at")?,
    })
}

fn row_to_cached_server(row: &sqlx::postgres::PgRow) -> RuntimeResult<CachedMcpServer> {
    Ok(CachedMcpServer {
        server_id: row.try_get("server_id")?,
        name: row.try_get("name")?,
        url: row.try_get("url")?,
        auth_token_ref: row.try_get("auth_token_ref")?,
        enabled: row.try_get("enabled")?,
    })
}

fn row_to_tool(row: &sqlx::postgres::PgRow) -> RuntimeResult<McpToolRecord> {
    Ok(McpToolRecord {
        server_id: row.try_get("server_id")?,
        tool_name: row.try_get("tool_name")?,
        description: row.try_get("description")?,
        input_schema: row.try_get("input_schema")?,
        enabled: row.try_get("enabled")?,
        updated_at: row.try_get("updated_at")?,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn agent_allow_list_supports_exact_and_wildcard_patterns() {
        let allowed = vec![
            "search".to_string(),
            "other:*".to_string(),
            "quire-report:open".to_string(),
        ];
        assert!(agent_allows_mcp_tool(&allowed, "server-a", "search"));
        assert!(agent_allows_mcp_tool(&allowed, "other", "anything"));
        assert!(agent_allows_mcp_tool(&allowed, "quire-report", "open"));
        assert!(!agent_allows_mcp_tool(&allowed, "server-a", "write"));
        assert!(agent_allows_mcp_tool(
            &["*".to_string()],
            "server-a",
            "write"
        ));
    }

    #[test]
    fn tool_patterns_support_prefix_suffix_and_multi_segment_wildcards() {
        assert!(tool_pattern_matches(
            "subjectcloud_*",
            "subjectcloud_detail"
        ));
        assert!(tool_pattern_matches(
            "shuangyou_intel_*",
            "shuangyou_intel_list"
        ));
        assert!(!tool_pattern_matches(
            "shuangyou_intel_*",
            "shuangyou_profile"
        ));
        assert!(!tool_pattern_matches(
            "subjectcloud_*",
            "panorama_report_query"
        ));
        assert!(tool_pattern_matches(
            "*_detail",
            "subjectcloud_talent_detail"
        ));
        assert!(tool_pattern_matches("*_detail", "shuangyou_intel_detail"));
        assert!(tool_pattern_matches("*_detail", "subjectcloud_detail"));
        assert!(!tool_pattern_matches("*_detail", "subjectcloud_overview"));
        assert!(tool_pattern_matches(
            "panorama_report_*_query",
            "panorama_report_batch_query"
        ));
        assert!(tool_pattern_matches("a*b*c", "axbyc"));
        assert!(!tool_pattern_matches("a*b*c", "axby"));
        assert!(tool_pattern_matches(" exact ", "exact"));
        assert!(!tool_pattern_matches("", "exact"));
        assert!(!tool_pattern_matches("   ", "exact"));
        assert!(tool_pattern_matches("*", "anything"));
    }

    #[test]
    fn scoped_wildcards_never_cross_server_boundaries() {
        let allowed = vec!["server_a:shuangyou_*".to_string()];
        assert!(agent_allows_mcp_tool(
            &allowed,
            "server_a",
            "shuangyou_profile"
        ));
        assert!(agent_allows_mcp_tool(
            &allowed,
            "server_a",
            "shuangyou_industry_source"
        ));
        assert!(!agent_allows_mcp_tool(
            &allowed,
            "server_b",
            "shuangyou_profile"
        ));
        assert!(!agent_allows_mcp_tool(
            &allowed,
            "server_a",
            "panorama_report_query"
        ));

        let all_on_server = vec!["server_a:*".to_string()];
        assert!(agent_allows_mcp_tool(
            &all_on_server,
            "server_a",
            "subjectcloud_result"
        ));
        assert!(!agent_allows_mcp_tool(
            &all_on_server,
            "server_b",
            "subjectcloud_result"
        ));

        let bare_prefix = vec!["subjectcloud_*".to_string()];
        assert!(agent_allows_mcp_tool(
            &bare_prefix,
            "any-server",
            "subjectcloud_overview"
        ));
        assert!(!agent_allows_mcp_tool(
            &bare_prefix,
            "any-server",
            "shuangyou_profile"
        ));
    }

    #[test]
    fn server_and_url_validation_rejects_unsafe_values() {
        assert!(validate_server_id("quire-report").is_ok());
        assert!(validate_server_id("../escape").is_err());
        assert!(validate_name_url("Quire", "http://127.0.0.1:8080").is_ok());
        assert!(validate_name_url("", "http://127.0.0.1").is_err());
        assert!(validate_name_url("Quire", "file:///tmp/socket").is_err());
    }
}
