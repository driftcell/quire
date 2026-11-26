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
        let active = summary.active_title.as_deref().unwrap_or("none");
        Some(format!(
            "Active todo: \"{active}\"; next pending: \"{next}\". Call update_todos when advancing, adding, or revising checklist steps."
        ))
    } else if let Some(active) = summary.active_title.as_deref() {
        Some(format!(
            "Final todo active (\"{active}\"). Mark all remaining todos completed (or skipped) via update_todos before finishing."
        ))
    } else {
        Some("All todos completed.".into())
    }
}

pub(super) fn update_todos_next_hint(
    summary: Option<&TodoProgressSummary>,
    has_live_focus: bool,
) -> Option<String> {
    let summary = summary?;
    let focus_suffix = if has_live_focus {
        ""
    } else {
        " Call describe_task to broadcast the specific action currently underway."
    };
    if let Some(next) = summary.next_pending_title.as_deref() {
        Some(format!(
            "Next todo: \"{next}\". Call update_todos when advancing or dynamically adjusting steps, and use describe_task for concrete live status.{focus_suffix}"
        ))
    } else if let Some(active) = summary.active_title.as_deref() {
        Some(format!(
            "Final todo active (\"{active}\"). Mark all remaining todos completed (or skipped) via update_todos before finishing.{focus_suffix}"
        ))
    } else {
        Some("All todos completed.".into())
    }
}

/// Broadcast what you are actively doing right now to the user interface (`title`, optional `detail`,
/// optional `section_id`).
///
/// Purpose & boundaries:
/// - ONLY reports live current work focus — it NEVER adds, completes, or modifies your todo checklist
///   (use `update_todos` to create, advance, or dynamically revise todo steps).
/// - `title` does NOT need to match any todo item; describe the exact concrete action underway right
///   now instead of leaving a coarse stage name frozen.
/// - May be called in the same parallel tool batch alongside the action it describes (`spawn_subagents`,
///   `upsert_sources`, `apply_operations`, search/fetch, `preview_echarts`, `lint_document`, etc.)
///   so status reporting costs zero extra turns.
///
/// When to call (both lead agent and subagents):
/// 1. When starting any concrete sub-task or shifting focus inside a stage (dispatching research
///    tracks, cross-verifying conflicting figures, restructuring sections, generating a chart,
///    drafting/humanizing prose, or fixing reviewer findings).
/// 2. Immediately after subagents return when synthesizing results or launching follow-up work.
/// 3. Before substantial edits in a specific report chapter (`section_id`), so the UI highlights and
///    links to the active section.
///
/// When NOT to call:
/// - Do not call only to update or complete checklist items — call `update_todos` for checklist changes.
/// - Do not re-send identical `title` and `detail` when the active focus has not changed.
#[rig_tool(name = "describe_task")]
pub(crate) async fn describe_task_tool(
    #[rig(context)] context: &mut ToolContext,
    /// Short, concrete Chinese status phrase (`<= 20` chars) describing the exact work happening now,
    /// with zero internal tool names or raw block IDs. Need not match a todo title.
    /// Good: `"交叉核验省属高校国自然口径"` or `"精修第三章师资对比段落"`;
    /// Bad: `"调用 apply_operations 更新 sec_2"` or repeating an unchanged macro stage title for 20 minutes.
    title: String,
    /// Optional concise Chinese note on current focus, data scope, or blocker (e.g. `"比对2021—2025年面上与青年项目增量冲突"`).
    detail: Option<String>,
    /// Optional `block_id` of the report heading currently being worked on to highlight the active chapter in the UI.
    section_id: Option<String>,
) -> Result<DescribeTaskReceipt, ToolExecutionError> {
    let runtime = runtime_tool_scope(context)?;
    let title = title.trim().to_string();
    if title.is_empty() {
        return Err(ToolExecutionError::invalid_args(
            "`title` must be a non-empty status phrase",
        ));
    }
    let detail = normalize_optional_text(detail);
    let section_id = normalize_optional_text(section_id);

    runtime.set_task_focus_title(&title);
    runtime
        .event_scope
        .send(ModelStreamEvent::TaskFocus {
            title: title.clone(),
            detail: detail.clone(),
            section_id: section_id.clone(),
        })
        .await;

    let current_todos = runtime.task_todo_snapshot();
    let summary = summarize_todo_progress(&current_todos);
    let _next_hint = describe_task_next_hint(summary.as_ref());
    let (active_todo, todo_progress) = match summary {
        Some(item) => (item.active_title, Some(item.progress)),
        None => (None, None),
    };
    Ok(DescribeTaskReceipt {
        title,
        detail,
        section_id,
        active_todo,
        todo_progress,
        _next_hint,
    })
}

/// Create, advance, or dynamically revise your structured stage checklist (`todos`) in the UI.
///
/// Purpose & boundaries:
/// - ONLY manages the ordered multi-step checklist (`todos`) — it NEVER replaces live action
///   narration (`describe_task`). Call `describe_task` alongside or between stage updates whenever
///   you begin a concrete sub-action inside a stage.
/// - Always pass the complete ordered `todos` list so dynamic additions, removals, reorderings, and
///   status transitions remain atomic and race-free even when called in parallel with `describe_task`.
/// - Keep items at workflow / chapter granularity (lead agent: 3–6 macro stages; subagents: 2–4
///   steps for multi-step work; parallel 8–16 char Chinese verb-object phrases), with at most one
///   `in_progress` item at a time.
///
/// Human-facing text contract:
/// - Every `title` and `detail` is rendered directly to people. Use concise natural Chinese business
///   language that explains the evidence, chapter, or outcome being advanced.
/// - NEVER expose tool/function names, MCP/module names, raw block/section/run/session IDs, revision
///   numbers, JSON keys, enum values, or other internal system identifiers in `title` or `detail`.
/// - Good title: `核验高校立项口径`. Good detail: `统一2021—2025年项目类型与时间窗`.
/// - Bad title: `调用 apply_operations 更新 sec_2`. Bad detail: `run_8 已完成 rev=3`.
///
/// When to call (both lead agent and subagents):
/// 1. **At task start**: initialize the checklist (item 1 = `in_progress`, remaining = `pending`).
/// 2. **At stage transitions**: immediately as you move to a new stage (before calling
///    `spawn_subagents` for a new phase, and after subagents return when advancing), mark finished
///    items `completed` and set the next item `in_progress`.
/// 3. **When the plan changes dynamically**: insert newly discovered steps (e.g. supplementary
///    evidence verification, outline restructuring, post-review revision), split/reorder upcoming
///    items, or mark obsolete steps `skipped`.
/// 4. **Before finishing**: mark all remaining checklist items `completed` (or `skipped`).
///
/// When NOT to call:
/// - Do not call for micro-actions inside an unchanged stage — use `describe_task` instead.
/// - Skip `update_todos` for a trivial 1–2 tool single lookup that needs no checklist; use only
///   `describe_task` there.
#[rig_tool(name = "update_todos")]
pub(crate) async fn update_todos_tool(
    #[rig(context)] context: &mut ToolContext,
    /// Full ordered todo checklist (`status`: `"pending" | "in_progress" | "completed" | "skipped"`,
    /// defaults to `"pending"` when omitted). Pass the complete list so dynamic insertions,
    /// removals, reorderings, and status transitions stay exact.
    todos: Vec<TaskTodoInput>,
) -> Result<UpdateTodosReceipt, ToolExecutionError> {
    let runtime = runtime_tool_scope(context)?;
    let normalized: Vec<quire_session_protocol::TaskTodoItem> = todos
        .into_iter()
        .filter_map(|item| {
            let mut item = quire_session_protocol::TaskTodoItem::from(item);
            item.id = normalize_optional_text(item.id);
            item.title = item.title.trim().to_string();
            item.detail = normalize_optional_text(item.detail);
            item.section_id = normalize_optional_text(item.section_id);
            if item.title.is_empty() {
                None
            } else {
                Some(item)
            }
        })
        .collect();
    if normalized.is_empty() {
        return Err(ToolExecutionError::invalid_args(
            "`todos` must contain at least one item with a non-empty `title`",
        ));
    }

    let stored = runtime.replace_task_todos(normalized);
    runtime
        .event_scope
        .send(ModelStreamEvent::TodosUpdated {
            todos: stored.clone(),
        })
        .await;

    let current_focus = runtime.task_focus_title_snapshot();
    let summary = summarize_todo_progress(&stored);
    let _next_hint = update_todos_next_hint(summary.as_ref(), current_focus.is_some());
    let progress = summary.map(|item| item.progress);
    Ok(UpdateTodosReceipt {
        todos: stored,
        progress,
        current_focus,
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
    let actor = turn
        .display_name
        .as_deref()
        .map(str::trim)
        .filter(|name| !name.is_empty())
        .unwrap_or(&turn.agent_id)
        .to_string();
    Ok(quire_server::auth::RequestContext {
        session_id: Some(session_id),
        actor,
        user_id: turn.user_id,
        is_admin: false,
        preview_doc_id: None,
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
