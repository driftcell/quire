use super::*;

#[rig_tool(name = "load_skill")]
pub(crate) async fn load_skill_tool(
    #[rig(context)] context: &mut ToolContext,
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

#[rig_tool(name = "read_skill_resource")]
pub(crate) async fn read_skill_resource_tool(
    #[rig(context)] context: &mut ToolContext,
    skill: String,
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

#[rig_tool(
    name = "describe_task",
    params(
        title = "Short Chinese status phrase, ideally at most 16 characters.",
        detail = "Optional clarification of the current focus or blocker.",
        section_id = "Optional `block_id` of the report heading currently being worked on."
    )
)]
pub(crate) async fn describe_task_tool(
    #[rig(context)] context: &mut ToolContext,
    title: String,
    detail: Option<String>,
    section_id: Option<String>,
) -> Result<DescribeTaskReceipt, ToolExecutionError> {
    let runtime = runtime_tool_scope(context)?;
    let title = title.trim().to_string();
    if title.is_empty() {
        return Err(ToolExecutionError::invalid_args(
            "`title` must be a non-empty status phrase",
        ));
    }
    runtime
        .event_scope
        .send(ModelStreamEvent::TaskFocus {
            title: title.clone(),
            detail: detail.clone(),
            section_id: section_id.clone(),
        })
        .await;
    Ok(DescribeTaskReceipt {
        title,
        detail,
        section_id,
    })
}

#[rig_tool(name = "set_session_title")]
pub(crate) async fn set_session_title_tool(
    #[rig(context)] context: &mut ToolContext,
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

#[rig_tool(name = "ask_user")]
pub(crate) fn ask_user_tool(
    #[rig(context)] context: &mut ToolContext,
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
