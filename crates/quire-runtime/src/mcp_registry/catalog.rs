use super::*;

impl McpRegistry {
    pub(crate) async fn get_server_internal(
        &self,
        server_id: &str,
    ) -> RuntimeResult<Option<CachedMcpServer>> {
        let row = sqlx::query(
            "SELECT server_id, name, url, auth_token_ref, enabled FROM mcp_servers WHERE server_id = $1",
        )
        .bind(server_id)
        .fetch_optional(self.store.pool())
        .await?;
        row.as_ref().map(row_to_cached_server).transpose()
    }
    pub(crate) async fn catalog(&self) -> RuntimeResult<Arc<McpCatalogSnapshot>> {
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

    pub(crate) async fn after_mutation(&self) {
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
