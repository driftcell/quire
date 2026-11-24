use super::*;

#[test]
fn wildcard_allow_list_enables_every_builtin_tool() {
    let wildcard = vec!["*".to_string()];
    for tool in BUILTIN_TOOLS {
        assert!(
            allow_list_matches(&wildcard, tool),
            "wildcard should enable built-in tool `{tool}`"
        );
    }
    assert!(allow_list_matches(&wildcard, "apply_operations"));
}

#[test]
fn ask_user_schema_exposes_questions_array() {
    assert!(!ASK_USER_TOOL.description().trim().is_empty());
    let parameters = ASK_USER_TOOL.parameters();
    let properties = parameters
        .get("properties")
        .and_then(Value::as_object)
        .expect("ask_user schema should have properties");
    assert!(properties.contains_key("questions"));
    assert!(!properties.contains_key("_questions"));
    assert_eq!(
        parameters.get("required"),
        Some(&json!(["questions"])),
        "ask_user must require the `questions` array"
    );
}

#[test]
fn describe_task_schema_advertises_status_and_parameters() {
    let description = DESCRIBE_TASK_TOOL.description();
    assert!(description.contains("current work stage"));
    assert!(description.contains("user interface"));

    let parameters = DESCRIBE_TASK_TOOL.parameters();
    let properties = parameters
        .get("properties")
        .and_then(Value::as_object)
        .expect("describe_task schema should have properties");
    for name in ["title", "detail", "section_id", "todos"] {
        assert!(properties.contains_key(name), "missing `{name}`");
    }
    let title = properties
        .get("title")
        .and_then(|value| value.get("description"))
        .and_then(Value::as_str)
        .expect("title description");
    assert!(title.contains("16"));
    assert_eq!(parameters.get("required"), Some(&json!(["title"])));
}

#[test]
fn exact_allow_list_stays_scoped() {
    let allowed = vec!["spawn_subagents".to_string(), "lint_document".to_string()];
    assert!(allow_list_matches(&allowed, "spawn_subagents"));
    assert!(allow_list_matches(&allowed, "lint_document"));
    assert!(!allow_list_matches(&allowed, "ask_user"));
    assert!(!allow_list_matches(&[], "spawn_subagents"));
}

#[test]
fn wildcard_allow_list_supports_prefix_suffix_and_multi_segment_patterns() {
    let allowed = vec![
        "subjectcloud_*".to_string(),
        "shuangyou_intel_*".to_string(),
        "*_detail".to_string(),
        "panorama_report_*_query".to_string(),
    ];
    assert!(allow_list_matches(&allowed, "subjectcloud_overview"));
    assert!(allow_list_matches(&allowed, "shuangyou_intel_list"));
    assert!(allow_list_matches(&allowed, "subjectcloud_talent_detail"));
    assert!(allow_list_matches(&allowed, "panorama_report_batch_query"));
    assert!(!allow_list_matches(&allowed, "shuangyou_profile"));
    assert!(!allow_list_matches(&allowed, "panorama_report_trend"));
}

#[test]
fn warn_missing_allowed_tools_expands_wildcards_instead_of_literal_lookup() {
    let tool_map = BTreeSet::from([
        "subjectcloud_detail".to_string(),
        "subjectcloud_overview".to_string(),
        "shuangyou_profile".to_string(),
    ]);

    let valid = vec![
        "subjectcloud_*".to_string(),
        "server_a:subjectcloud_*".to_string(),
        "subjectcloud_detail".to_string(),
        "*".to_string(),
    ];
    assert!(
        warn_missing_allowed_tools("agent", &valid, &tool_map).is_empty(),
        "valid wildcards must not be reported as missing tools"
    );

    let missing = warn_missing_allowed_tools(
        "agent",
        &[
            "panorama_report_*".to_string(),
            "shuangyou_intel_*".to_string(),
        ],
        &tool_map,
    );
    assert_eq!(
        missing,
        vec!["panorama_report_*", "shuangyou_intel_*"],
        "only patterns with no matching assembled tool should warn"
    );
}

#[test]
fn sandbox_tool_schema_exposes_code_and_optional_limits() {
    assert!(!SANDBOX_EXECUTE_TOOL.description().trim().is_empty());
    let parameters = SANDBOX_EXECUTE_TOOL.parameters();
    let properties = parameters
        .get("properties")
        .and_then(Value::as_object)
        .expect("sandbox_execute schema should have properties");
    assert!(properties.contains_key("language"));
    assert!(properties.contains_key("code"));
    assert!(properties.contains_key("cwd"));
    assert!(properties.contains_key("timeout_seconds"));
    assert_eq!(
        parameters.get("required"),
        Some(&json!(["language", "code"]))
    );
}

#[test]
fn sandbox_mount_attachment_schema_requires_attachment_id() {
    assert!(
        !SANDBOX_MOUNT_ATTACHMENT_TOOL
            .description()
            .trim()
            .is_empty()
    );
    let parameters = SANDBOX_MOUNT_ATTACHMENT_TOOL.parameters();
    let properties = parameters
        .get("properties")
        .and_then(Value::as_object)
        .expect("sandbox_mount_attachment schema should have properties");
    assert!(properties.contains_key("attachment_id"));
    assert_eq!(parameters.get("required"), Some(&json!(["attachment_id"])));
}

#[test]
fn oss_download_errors_never_expose_signed_query_values() {
    let url = "https://agentcore-ws-test.oss-cn-hangzhou.aliyuncs.com/quire/attachments/sess/att/data.csv?x-oss-credential=LTAI_TEST%2F20261120%2Fcn-hangzhou%2Foss%2Faliyun_v4_request&x-oss-date=20261120T080000Z&x-oss-expires=600&x-oss-signature-version=OSS4-HMAC-SHA256&x-oss-signature=deadbeef";
    let stderr = format!(
        "curl: (22) The requested URL returned error: 403\n{url}\nx-oss-signature=deadbeef"
    );
    let sanitized = sanitize_oss_download_error(&stderr, url);
    assert!(sanitized.contains("[REDACTED_OSS_URL]"));
    assert!(!sanitized.contains("x-oss-signature="));
    assert!(!sanitized.contains("x-oss-credential="));
    assert!(!sanitized.contains("deadbeef"));
    assert!(!sanitized.contains("LTAI_TEST"));
}

#[test]
fn shell_quote_handles_quotes_and_shell_metacharacters() {
    assert_eq!(shell_quote("data.csv"), "'data.csv'");
    assert_eq!(shell_quote("a'b&c"), "'a'\\''b&c'");
}

#[test]
fn chart_preview_tools_enforce_the_no_repeat_option_contract() {
    assert!(!PREVIEW_ECHARTS_TOOL.description().trim().is_empty());
    let preview = PREVIEW_ECHARTS_TOOL.parameters();
    let preview_properties = preview
        .get("properties")
        .and_then(Value::as_object)
        .expect("preview_echarts schema should have properties");
    assert!(preview_properties.contains_key("option"));
    assert!(preview_properties.contains_key("option_patch"));
    assert!(preview_properties.contains_key("based_on_preview_id"));

    assert!(!COMMIT_CHART_TOOL.description().trim().is_empty());
    let commit = COMMIT_CHART_TOOL.parameters();
    let commit_properties = commit
        .get("properties")
        .and_then(Value::as_object)
        .expect("commit_chart schema should have properties");
    assert!(commit_properties.contains_key("preview_id"));
    assert!(commit_properties.contains_key("doc_id"));
    assert!(commit_properties.contains_key("block_id"));
    assert!(!commit_properties.contains_key("option"));
    assert!(!commit_properties.contains_key("option_patch"));
}

#[test]
fn bounded_tool_text_preserves_utf8_boundaries() {
    let (text, truncated) = bounded_tool_text("好".repeat(MAX_TOOL_OUTPUT_BYTES));
    assert!(truncated);
    assert!(text.is_char_boundary(text.len()));
    assert!(text.len() <= MAX_TOOL_OUTPUT_BYTES);
}

#[test]
fn native_report_tools_expose_non_empty_metadata_and_match_allow_lists() {
    for name in quire_server::mcp::REPORT_TOOL_NAMES {
        assert!(
            BUILTIN_TOOLS.contains(name),
            "report tool `{name}` must be registered in BUILTIN_TOOLS"
        );
        let (description, schema) = quire_server::mcp::QuireMcpServer::tool_metadata(name);
        assert!(
            !description.trim().is_empty(),
            "report tool `{name}` must expose a non-empty description"
        );
        assert_eq!(
            schema.get("type").and_then(Value::as_str),
            Some("object"),
            "report tool `{name}` must expose an object JSON Schema"
        );
        assert!(allow_list_matches(&[name.to_string()], name));
        assert!(allow_list_matches(&[format!("quire-report:{name}")], name));
        assert!(allow_list_matches(&["quire-report:*".to_string()], name));
    }
    assert!(!allow_list_matches(
        &["quire-report:*".to_string()],
        "ask_user"
    ));
}
