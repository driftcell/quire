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

pub(crate) struct TodoProgressSummary {
    pub(crate) progress: String,
    pub(crate) active_title: Option<String>,
    pub(crate) next_pending_title: Option<String>,
}

/// Summarize current todo completion counts alongside active and next pending item titles.
pub(crate) fn summarize_todo_progress(
    todos: &[quire_session_protocol::TaskTodoItem],
) -> Option<TodoProgressSummary> {
    if todos.is_empty() {
        return None;
    }
    let total = todos.len();
    let mut completed = 0usize;
    let mut in_progress = 0usize;
    let mut pending = 0usize;
    let mut active_title = None;
    let mut next_pending_title = None;

    for item in todos {
        match item.status {
            quire_session_protocol::TaskTodoStatus::Completed => completed += 1,
            quire_session_protocol::TaskTodoStatus::InProgress => {
                in_progress += 1;
                if active_title.is_none() {
                    active_title = Some(item.title.clone());
                }
            }
            quire_session_protocol::TaskTodoStatus::Pending => {
                pending += 1;
                if next_pending_title.is_none() {
                    next_pending_title = Some(item.title.clone());
                }
            }
            quire_session_protocol::TaskTodoStatus::Skipped => {}
        }
    }

    let mut progress =
        format!("{completed}/{total} completed, {in_progress} in_progress, {pending} pending");
    if let Some(active) = active_title.as_deref() {
        progress.push_str(&format!(" (active: \"{active}\""));
        if let Some(next) = next_pending_title.as_deref() {
            progress.push_str(&format!("; next: \"{next}\""));
        }
        progress.push(')');
    } else if let Some(next) = next_pending_title.as_deref() {
        progress.push_str(&format!(" (next: \"{next}\")"));
    }

    Some(TodoProgressSummary {
        progress,
        active_title,
        next_pending_title,
    })
}

pub(super) fn describe_task_next_hint(summary: Option<&TodoProgressSummary>) -> Option<String> {
    let summary = summary?;
    if let Some(next) = summary.next_pending_title.as_deref() {
        Some(format!(
            "Next todo: \"{next}\". Call describe_task(title=\"{next}\") when transitioning to that stage."
        ))
    } else if let Some(active) = summary.active_title.as_deref() {
        Some(format!(
            "Final todo active (\"{active}\"). Mark all todos completed with describe_task before finishing."
        ))
    } else {
        Some("All todos completed.".into())
    }
}

/// Report the current work stage and step-by-step todo progress to the user interface.
///
/// When to call (both lead agent and subagents):
/// 1. At task/phase start: initialize `todos` (item 1 = `in_progress`, remaining = `pending`).
/// 2. At EVERY step transition — immediately as you move from one todo item to the next, before
///    calling `spawn_subagents`, and after subagents return — so progress advances one item at a time
///    instead of batch-completing everything at the end.
/// 3. Fast incremental step: omit `todos` and pass `title` matching the next todo item; the runtime
///    automatically marks the previous `in_progress` item `completed` and activates the matching item.
///    Pass `todos` only when initializing, adding/reordering items, or marking the final item `completed`.
/// 4. Before finishing: mark all remaining items `completed` (or `skipped`).
#[rig_tool(name = "describe_task")]
pub(crate) async fn describe_task_tool(
    #[rig(context)] context: &mut ToolContext,
    /// Short Chinese status phrase (`<= 16` chars) with zero internal tool names or raw IDs.
    /// When advancing an existing todo list without `todos`, match the next todo item's `title`.
    /// Good: `"核对省属高校国自然立项口径"`; Bad: `"调用 apply_operations 更新 sec_2"`.
    title: String,
    /// Optional concise Chinese note on the current focus or blocker (e.g. `"对比2021—2025年面上与青年项目增量"`).
    detail: Option<String>,
    /// Optional `block_id` of the report heading currently being worked on to highlight the active chapter in the UI.
    section_id: Option<String>,
    /// Ordered todo list (`status`: `"pending" | "in_progress" | "completed" | "skipped"`). Omit when
    /// advancing sequentially by `title`. Keep titles as parallel 8–16 char Chinese verb-object phrases
    /// with at most one `in_progress` item at a time.
    todos: Option<Vec<TaskTodoInput>>,
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
        .filter_map(|item| {
            let mut item = quire_session_protocol::TaskTodoItem::from(item);
            item.title = item.title.trim().to_string();
            if item.title.is_empty() {
                None
            } else {
                Some(item)
            }
        })
        .collect();
    let effective_todos = runtime.update_task_todos(
        &title,
        detail.as_deref(),
        section_id.as_deref(),
        &normalized_todos,
    );
    runtime
        .event_scope
        .send(ModelStreamEvent::TaskFocus {
            title: title.clone(),
            detail: detail.clone(),
            section_id: section_id.clone(),
            todos: normalized_todos,
        })
        .await;
    let summary = summarize_todo_progress(&effective_todos);
    let _next_hint = describe_task_next_hint(summary.as_ref());
    let progress = summary.map(|item| item.progress);
    Ok(DescribeTaskReceipt {
        title,
        detail,
        section_id,
        todos: effective_todos,
        progress,
        _next_hint,
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
/// (sibling tool calls in the same turn are skipped). Available only to the lead agent when scope or
/// direction is genuinely ambiguous and cannot be inferred from context.
#[rig_tool(name = "ask_user")]
pub(crate) fn ask_user_tool(
    #[rig(context)] context: &mut ToolContext,
    /// Non-empty list of 1–4 structured questions. Each question needs a concise Chinese `title` and concrete `options` with the recommended choice first (the UI provides custom input automatically, so do not add a filler `"其他"` option).
    /// Good `title`: `"本次对标分析优先选取哪组参照高校？"`; Bad: `"请问您有什么具体要求？"`.
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
