use super::*;

impl RuntimeService {
    pub(crate) async fn execute_turn(
        &self,
        session_id: &str,
        run_id: &str,
        content: &str,
        _current_message_id: Option<&str>,
        token: &CancellationToken,
    ) -> RuntimeResult<()> {
        let session = self.store.get_session_summary(session_id).await?;
        let (pinned, synced_version) = self.ensure_pinned_prompt(session_id).await?;
        let kb_version = self.kb.current_version().await?;
        let kb_update_notice = (kb_version > synced_version).then(|| {
            format!(
                "<knowledge_update>Knowledge base advanced from version {synced_version} to {kb_version}. Call load_skill or read_skill_resource before relying on skill-specific guidance.</knowledge_update>"
            )
        });
        if token.is_cancelled() {
            self.finish_cancelled_run(run_id).await?;
            return Ok(());
        }
        let mut sink = EventSink::new(session_id, self.store.clone());
        let main_scope = EventScope::main(run_id);
        self.store
            .start_run_with_event(run_id, SessionEventKind::TurnStarted)
            .await?;
        if token.is_cancelled() {
            self.finish_cancelled_run(run_id).await?;
            return Ok(());
        }
        let branch = session.kb_branch.clone();
        let agent = self.kb.get_default_agent_on(branch.as_deref()).await?;
        let model_route = agent
            .as_ref()
            .map(|agent| agent.model.clone())
            .unwrap_or_else(|| self.config.default_model.clone());
        let allowed_tools = agent.map(|agent| agent.allowed_tools).unwrap_or_default();
        let request = ModelTurnRequest {
            session_id: session_id.to_string(),
            run_id: run_id.to_string(),
            agent_id: "main".into(),
            system_prompt: pinned.clone(),
            user_message: content.to_string(),
            kb_update_notice,
            doc_id: session.doc_id.clone(),
            kb_branch: branch.clone(),
            model_route: model_route.clone(),
            allowed_tools,
            cancellation: token.clone(),
            prompt_recorded: true,
            persist_history: true,
        };
        let main_resolved_model = self.models.resolve(&model_route).await.ok();
        let mut cumulative_cost_rmb = session.context_usage.cost_rmb;
        let mut latest_main_usage = session.context_usage.clone();
        let mut stream = self.model_stream.stream_turn(request).await?;
        let mut tick = tokio::time::interval(Duration::from_millis(50));
        let mut response = String::new();
        let mut reported_usage: Option<ContextUsage> = None;
        let mut suspended_question: Option<PendingQuestion> = None;
        let mut superseded = false;
        loop {
            tokio::select! {
                _ = tick.tick() => {
                    sink.flush_due(Instant::now()).await?;
                }
                item = stream.next() => {
                    match item {
                        Some(Ok(ModelStreamEvent::Superseded)) => {
                            superseded = true;
                        }
                        Some(Ok(event)) => {
                            let mut action = project(main_scope.clone(), event);
                            if let Some(text) = action.main_text_delta() {
                                response.push_str(text);
                            }
                            let mut subagent_cost_updated = false;
                            if let crate::events::EventAction::Usage { scope, usage } = &mut action {
                                if usage.cost_rmb.is_none()
                                    && let Some(resolved) = main_resolved_model.as_ref()
                                {
                                    usage.cost_rmb = resolved.calculate_cost_rmb(
                                        usage.input_tokens,
                                        usage.output_tokens,
                                        usage.cached_tokens,
                                    );
                                }
                                if let Some(call_cost) = usage.cost_rmb {
                                    cumulative_cost_rmb =
                                        Some(cumulative_cost_rmb.unwrap_or(0.0) + call_cost);
                                }
                                usage.cost_rmb = cumulative_cost_rmb;
                                if !scope.is_main() && cumulative_cost_rmb.is_some() {
                                    latest_main_usage.cost_rmb = cumulative_cost_rmb;
                                    if let Some(rep) = reported_usage.as_mut() {
                                        rep.cost_rmb = cumulative_cost_rmb;
                                    }
                                    subagent_cost_updated = true;
                                }
                            }
                            let outcome = sink.apply(action).await?;
                            if let Some(reason) = outcome.ignored {
                                warn!(%session_id, %run_id, reason, "ignoring out-of-scope model event");
                            }
                            if let Some(usage) = outcome.main_usage {
                                latest_main_usage = usage.clone();
                                reported_usage = Some(usage);
                                self.store
                                    .update_context_usage(session_id, &latest_main_usage)
                                    .await?;
                            } else if subagent_cost_updated {
                                self.store
                                    .update_context_usage(session_id, &latest_main_usage)
                                    .await?;
                            }
                            if let Some(question) = outcome.question {
                                suspended_question = Some(question);
                            }
                        }
                        Some(Err(error)) => {
                            sink.flush().await?;
                            if !response.is_empty()
                                && let Err(persist_error) = self
                                    .store
                                    .insert_assistant_message(session_id, &response, run_id)
                                    .await
                            {
                                warn!(%persist_error, %session_id, %run_id, "failed to preserve assistant text after turn failure");
                            }
                            return Err(error);
                        }
                        None => break,
                    }
                }
            }
        }
        sink.flush().await?;
        if token.is_cancelled() {
            self.finish_cancelled_run(run_id).await?;
            return Ok(());
        }
        if !response.is_empty() && !self.model_stream.commits_canonical_history() {
            self.store
                .insert_assistant_message(session_id, &response, run_id)
                .await?;
        }
        if let Some(question) = suspended_question {
            if let Some(usage) = reported_usage {
                self.store.update_context_usage(session_id, &usage).await?;
            }
            self.store.mark_run_awaiting_input(run_id).await?;
            let _ = question;
            return Ok(());
        }
        if superseded {
            if let Some(usage) = reported_usage {
                self.store.update_context_usage(session_id, &usage).await?;
            }
            let _ = self
                .store
                .finish_run_with_event(run_id, "superseded", None, SessionEventKind::TurnSuperseded)
                .await?;
            return Ok(());
        }
        let usage = match reported_usage {
            Some(mut usage) => {
                usage.cost_rmb = cumulative_cost_rmb;
                usage
            }
            None => {
                let mut usage = self
                    .fallback_usage(&model_route, &pinned, content, &response)
                    .await;
                if let Some(call_cost) = usage.cost_rmb {
                    cumulative_cost_rmb = Some(cumulative_cost_rmb.unwrap_or(0.0) + call_cost);
                }
                usage.cost_rmb = cumulative_cost_rmb;
                sink.emit(
                    main_scope.clone(),
                    SessionEventKind::ContextUsage {
                        input_tokens: usage.input_tokens,
                        output_tokens: usage.output_tokens,
                        cached_tokens: usage.cached_tokens,
                        context_used: usage.context_used,
                        context_limit: usage.context_limit,
                        cost_rmb: usage.cost_rmb,
                    },
                )
                .await?;
                usage
            }
        };
        self.store.update_context_usage(session_id, &usage).await?;
        if token.is_cancelled() {
            self.finish_cancelled_run(run_id).await?;
            return Ok(());
        }
        let Some(_terminal) = self
            .store
            .finish_run_with_event(run_id, "completed", None, SessionEventKind::TurnFinished)
            .await?
        else {
            // A user interrupt won the terminal transition; do not emit a second
            // terminal event or advance the session as a completed run.
            return Ok(());
        };
        if kb_version > synced_version {
            self.store
                .advance_synced_kb_version(session_id, kb_version)
                .await?;
        }
        Ok(())
    }

    async fn fallback_usage(
        &self,
        model_route: &str,
        system_prompt: &str,
        content: &str,
        response: &str,
    ) -> ContextUsage {
        let resolved = self.models.resolve(model_route).await.ok();
        let context_limit = match resolved.as_ref() {
            Some(model) => u64::try_from(model.context_limit).unwrap_or(0),
            None => {
                warn!(
                    model = model_route,
                    "model route unavailable; using offline context limit"
                );
                200_000
            }
        };
        let input_tokens = u64::try_from(content.chars().count() / 4).unwrap_or(0);
        let output_tokens = u64::try_from(response.chars().count() / 4).unwrap_or(0);
        let cached_tokens = u64::try_from(system_prompt.chars().count() / 4).unwrap_or(0);
        let cost_rmb = resolved
            .as_ref()
            .and_then(|model| model.calculate_cost_rmb(input_tokens, output_tokens, cached_tokens));
        ContextUsage {
            input_tokens,
            output_tokens,
            cached_tokens,
            context_used: u64::try_from(
                (system_prompt.chars().count()
                    + content.chars().count()
                    + response.chars().count())
                    / 4,
            )
            .unwrap_or(0),
            context_limit,
            cost_rmb,
        }
    }

    pub(crate) async fn emit(
        &self,
        session_id: &str,
        run_id: &str,
        agent_id: &str,
        parent_run_id: Option<&str>,
        kind: SessionEventKind,
    ) -> RuntimeResult<SessionEvent> {
        self.store
            .append_event(session_id, run_id, agent_id, parent_run_id, kind)
            .await
    }
}

pub(crate) fn validate_question_answers(
    questions: &[QuestionItem],
    answers: &[QuestionAnswer],
) -> RuntimeResult<()> {
    let questions: HashMap<&str, &QuestionItem> = questions
        .iter()
        .map(|question| (question.id.as_str(), question))
        .collect();
    for answer in answers {
        let question = questions.get(answer.question_id.as_str()).ok_or_else(|| {
            RuntimeError::InvalidRequest(format!(
                "answer references unknown question `{}`",
                answer.question_id
            ))
        })?;
        if answer.skipped {
            continue;
        }
        for option_id in &answer.selected_option_ids {
            if !question
                .options
                .iter()
                .any(|option| option.id == *option_id)
            {
                return Err(RuntimeError::InvalidRequest(format!(
                    "answer for `{}` references unknown option `{option_id}`",
                    answer.question_id
                )));
            }
        }
    }
    Ok(())
}

pub(crate) fn safe_attachment_id(attachment_id: &str) -> RuntimeResult<&str> {
    let trimmed = attachment_id.trim();
    if trimmed.is_empty()
        || trimmed.len() > 128
        || !trimmed
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'_' | b'-'))
    {
        return Err(RuntimeError::InvalidRequest(
            "attachment_id must contain only ASCII letters, digits, `_` or `-`".into(),
        ));
    }
    Ok(trimmed)
}

pub(crate) fn safe_attachment_filename(filename: &str) -> RuntimeResult<&str> {
    let trimmed = filename.trim();
    if trimmed.is_empty()
        || trimmed.contains("..")
        || trimmed.contains('/')
        || trimmed.contains('\\')
        || trimmed.chars().any(char::is_control)
    {
        return Err(RuntimeError::InvalidRequest(
            "attachment filename must be a plain file name".into(),
        ));
    }
    let Some(name) = Path::new(trimmed)
        .file_name()
        .and_then(|name| name.to_str())
    else {
        return Err(RuntimeError::InvalidRequest(
            "attachment filename must be valid UTF-8".into(),
        ));
    };
    if name != trimmed {
        return Err(RuntimeError::InvalidRequest(
            "attachment filename must not contain a path".into(),
        ));
    }
    Ok(name)
}
