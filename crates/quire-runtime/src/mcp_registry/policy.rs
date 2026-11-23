use super::*;

pub(crate) fn default_enabled() -> bool {
    true
}

pub(crate) fn validate_server_id(server_id: &str) -> RuntimeResult<()> {
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

pub(crate) fn validate_name_url(name: &str, url: &str) -> RuntimeResult<()> {
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

pub(crate) fn row_to_server(row: &sqlx::postgres::PgRow) -> RuntimeResult<McpServerRecord> {
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

pub(crate) fn row_to_cached_server(row: &sqlx::postgres::PgRow) -> RuntimeResult<CachedMcpServer> {
    Ok(CachedMcpServer {
        server_id: row.try_get("server_id")?,
        name: row.try_get("name")?,
        url: row.try_get("url")?,
        auth_token_ref: row.try_get("auth_token_ref")?,
        enabled: row.try_get("enabled")?,
    })
}

pub(crate) fn row_to_tool(row: &sqlx::postgres::PgRow) -> RuntimeResult<McpToolRecord> {
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
