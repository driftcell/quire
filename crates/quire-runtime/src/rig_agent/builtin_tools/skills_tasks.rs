use super::*;

/// List all enabled skills available on the current session's knowledge-base branch,
/// returning each skill's `name`, `description`, and bundled `resources` paths.
#[rig_tool(name = "list_skills")]
pub(crate) async fn list_skills_tool(
    #[rig(context)] context: &mut ToolContext,
) -> Result<ListSkillsReceipt, ToolExecutionError> {
    let runtime = runtime_tool_scope(context)?;
    let turn = quire_turn_meta(context)?;
    let skills = runtime
        .engine
        .kb
        .list_skills_on(turn.kb_branch.as_deref())
        .await
        .map_err(tool_other)?
        .into_iter()
        .filter(|skill| skill.enabled)
        .map(|skill| SkillSummaryView {
            name: skill.name,
            description: skill.description,
            resources: skill.resources.into_iter().map(|r| r.path).collect(),
        })
        .collect();
    Ok(ListSkillsReceipt { skills })
}

/// Load a skill definition (`SKILL.md` body and list of bundled resource file paths) from the
/// session's knowledge-base branch. Follow up with `read_skill_resource` to read specific
/// bundled reference files listed in the skill.
#[rig_tool(name = "load_skill")]
pub(crate) async fn load_skill_tool(
    #[rig(context)] context: &mut ToolContext,
    /// Skill name on the active session KB branch (e.g. `"higher-education-writing"`).
    name: String,
) -> Result<crate::kb::SkillDefinitionRecord, ToolExecutionError> {
    let runtime = runtime_tool_scope(context)?;
    let turn = quire_turn_meta(context)?;
    let skill = runtime
        .engine
        .kb
        .get_skill_on(turn.kb_branch.as_deref(), &name)
        .await
        .map_err(tool_other)?
        .ok_or_else(|| ToolExecutionError::not_found(format!("skill `{name}`")))?;
    Ok(skill)
}

/// Read one bundled resource file belonging to a skill on the session's knowledge-base branch.
/// Load only the resource files relevant to your active role and task stage.
#[rig_tool(name = "read_skill_resource")]
pub(crate) async fn read_skill_resource_tool(
    #[rig(context)] context: &mut ToolContext,
    /// Skill name that owns the bundled resource (e.g. `"higher-education-writing"`).
    skill: String,
    /// Relative path of the resource file inside the skill (e.g. `"references/examples.md"` or `"references/review-rubric.md"`).
    path: String,
) -> Result<ReadSkillResourceReceipt, ToolExecutionError> {
    let runtime = runtime_tool_scope(context)?;
    let turn = quire_turn_meta(context)?;
    let definition = runtime
        .engine
        .kb
        .get_skill_on(turn.kb_branch.as_deref(), &skill)
        .await
        .map_err(tool_other)?
        .ok_or_else(|| ToolExecutionError::not_found(format!("skill `{skill}`")))?;
    let resource = definition
        .resources
        .iter()
        .find(|resource| resource.path == path)
        .ok_or_else(|| ToolExecutionError::not_found(format!("resource `{path}` in `{skill}`")))?;
    Ok(ReadSkillResourceReceipt {
        skill,
        path,
        content: resource.content.clone(),
    })
}

/// Report the current work stage and dynamic todo-list plan to the user interface without cluttering the tool trace.
/// Both the lead agent and subagents should pass and dynamically update `todos` at phase transitions so the UI can render an accurate step-by-step todo list and progress bar.
#[rig_tool(name = "describe_task")]
pub(crate) async fn describe_task_tool(
    #[rig(context)] context: &mut ToolContext,
    /// Short Chinese status phrase for the active stage, ideally at most 16 characters (no internal tool names or raw IDs).
    title: String,
    /// Optional clarification of the current focus or blocker.
    detail: Option<String>,
    /// Optional `block_id` of the report heading currently being worked on to highlight the active chapter in the UI.
    section_id: Option<String>,
    /// Optional ordered todo list for this agent (`status`: `"pending" | "in_progress" | "completed" | "skipped"`). Pass the full planned todo list on start and dynamically adjust statuses or items as stages complete.
    todos: Option<Vec<quire_session_protocol::TaskTodoItem>>,
) -> Result<DescribeTaskReceipt, ToolExecutionError> {
    let runtime = runtime_tool_scope(context)?;
    let title = title.trim().to_string();
    if title.is_empty() {
        return Err(ToolExecutionError::invalid_args(
            "`title` must be a non-empty status phrase",
        ));
    }
    let normalized_todos: Vec<quire_session_protocol::TaskTodoItem> = todos
        .unwrap_or_default()
        .into_iter()
        .filter_map(|mut item| {
            item.title = item.title.trim().to_string();
            if item.title.is_empty() {
                None
            } else {
                Some(item)
            }
        })
        .collect();
    runtime
        .event_scope
        .send(ModelStreamEvent::TaskFocus {
            title: title.clone(),
            detail: detail.clone(),
            section_id: section_id.clone(),
            todos: normalized_todos.clone(),
        })
        .await;
    Ok(DescribeTaskReceipt {
        title,
        detail,
        section_id,
        todos: normalized_todos,
    })
}

/// Set or refine the current chat session title (automatically trimmed and capped at 60 characters)
/// once the user's topic is clear. Available only to the lead agent; call once in the opening turn.
#[rig_tool(name = "set_session_title")]
pub(crate) async fn set_session_title_tool(
    #[rig(context)] context: &mut ToolContext,
    /// Concise human-readable session title (non-empty, automatically capped at 60 characters).
    title: String,
) -> Result<SetSessionTitleReceipt, ToolExecutionError> {
    let runtime = runtime_tool_scope(context)?;
    let turn = quire_turn_meta(context)?;
    let title: String = title.trim().chars().take(60).collect();
    if title.is_empty() {
        return Err(ToolExecutionError::invalid_args(
            "`title` must be non-empty",
        ));
    }
    runtime
        .engine
        .kb
        .store()
        .update_session(
            &turn.session_id,
            SessionPatch {
                title: Some(&title),
                ..SessionPatch::default()
            },
        )
        .await
        .map_err(tool_other)?;
    runtime
        .event_scope
        .send(ModelStreamEvent::SessionTitleUpdated {
            title: title.clone(),
        })
        .await;
    Ok(SetSessionTitleReceipt {
        session_id: turn.session_id,
        title,
    })
}

/// Present a one-shot structured questionnaire to the user and immediately suspend the current turn
/// (any sibling tool calls in the same model turn are skipped without execution). Available only
/// to the lead agent.
///
/// Reserve this for genuinely ambiguous scope or direction decisions that cannot be resolved from
/// the user's prompt, existing workspace data, or sensible defaults.
#[rig_tool(name = "ask_user")]
pub(crate) fn ask_user_tool(
    #[rig(context)] context: &mut ToolContext,
    /// Non-empty list of structured questions; each question must provide a non-empty `id`, `title`, and at least one preset option in `options` (place your recommended option first).
    questions: Vec<QuestionItem>,
) -> Result<AskUserReceipt, ToolExecutionError> {
    if questions.is_empty() {
        return Err(ToolExecutionError::invalid_args(
            "`ask_user` requires at least one question item",
        ));
    }
    for item in &questions {
        if item.id.trim().is_empty() || item.title.trim().is_empty() {
            return Err(ToolExecutionError::invalid_args(
                "every question item must have a non-empty `id` and `title`",
            ));
        }
        if item.options.is_empty() {
            return Err(ToolExecutionError::invalid_args(format!(
                "question `{}` must provide at least one option",
                item.id
            )));
        }
    }
    context
        .insert_result(AskUserRequestedMeta {
            questions: questions.clone(),
        })
        .map_err(|error| ToolExecutionError::other(error.to_string()))?;
    Ok(AskUserReceipt {
        status: "awaiting_input",
        question_count: questions.len(),
    })
}

/// List enabled subagent definitions available on the current session's knowledge-base branch,
/// returning each subagent's `name`, `description`, default `model`, and `allowed_tools` patterns.
/// Excludes the default lead agent and the calling agent itself.
#[rig_tool(name = "list_subagents")]
pub(crate) async fn list_subagents_tool(
    #[rig(context)] context: &mut ToolContext,
) -> Result<ListSubagentsReceipt, ToolExecutionError> {
    let runtime = runtime_tool_scope(context)?;
    let turn = quire_turn_meta(context)?;
    let agents = runtime
        .engine
        .kb
        .list_agents_on(turn.kb_branch.as_deref())
        .await
        .map_err(tool_other)?;
    let subagents = agents
        .into_iter()
        .filter(|agent| agent.enabled && !agent.is_default && agent.name != turn.agent_id)
        .map(|agent| SubagentSummaryView {
            name: agent.name,
            description: agent.description,
            model: agent.model,
            allowed_tools: agent.allowed_tools,
        })
        .collect();
    Ok(ListSubagentsReceipt { subagents })
}

pub(crate) fn in_process_request_context(
    context: &ToolContext,
) -> Result<quire_server::auth::RequestContext, ToolExecutionError> {
    let turn = quire_turn_meta(context)?;
    let session_id = quire_protocol::SessionId::parse(turn.session_id)
        .map_err(|error| ToolExecutionError::invalid_args(error.to_string()))?;
    Ok(quire_server::auth::RequestContext {
        session_id: Some(session_id),
        actor: turn.agent_id,
    })
}

pub(crate) fn mcp_error_to_tool_error(error: rmcp::ErrorData) -> ToolExecutionError {
    let formatted = error
        .data
        .as_ref()
        .and_then(|data| serde_json::to_string(data).ok())
        .unwrap_or_else(|| error.message.to_string());
    if error.code == rmcp::model::ErrorCode::INVALID_PARAMS {
        ToolExecutionError::invalid_args(formatted)
    } else {
        ToolExecutionError::other(formatted)
    }
}
