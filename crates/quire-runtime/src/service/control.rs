use super::*;

impl RuntimeService {
    pub async fn interrupt(
        &self,
        session_id: &str,
        run_id: Option<&str>,
    ) -> RuntimeResult<SessionDetail> {
        let run = match run_id {
            Some(run_id) => self.store.get_run(run_id).await?,
            None => self
                .store
                .latest_running_run(session_id)
                .await?
                .ok_or_else(|| RuntimeError::InvalidRequest("session has no active run".into()))?,
        };
        if run.session_id != session_id {
            return Err(RuntimeError::InvalidRequest(
                "run does not belong to this session".into(),
            ));
        }
        let _ = self.runs.cancel(&run.run_id).await;
        if let Err(error) = self
            .store
            .bus()
            .publish_interrupt(&InterruptCommand {
                session_id: session_id.to_string(),
                run_id: run.run_id.clone(),
            })
            .await
        {
            warn!(%error, %session_id, run_id = %run.run_id, "failed to publish cross-instance interrupt; continuing local cancellation");
        }

        // Let the owning turn flush its remaining deltas and finish itself. Only
        // a bounded fallback writes the terminal event here; the conditional
        // transition guarantees there is exactly one winner and one terminal frame.
        let mut finalized = false;
        for _ in 0..40 {
            if self.store.get_run(&run.run_id).await?.status != "running" {
                finalized = true;
                break;
            }
            tokio::time::sleep(Duration::from_millis(50)).await;
        }
        if !finalized {
            self.finish_cancelled_run(&run.run_id).await?;
        }
        self.runs.remove(&run.run_id).await;
        self.store.get_session_detail(session_id).await
    }

    pub async fn retry(&self, session_id: &str) -> RuntimeResult<SessionDetail> {
        let gate = self.session_gate(session_id).await;
        let _guard = gate.lock_owned().await;
        let messages = self.store.list_messages(session_id).await?;
        let Some(last_user) = messages
            .iter()
            .rev()
            .find(|message| message.role == MessageRole::User)
            .cloned()
        else {
            return Err(RuntimeError::InvalidRequest(
                "session has no user message to retry".into(),
            ));
        };
        let prompt = self
            .store
            .memory()
            .await?
            .rewind_to_user_prompt(session_id, &last_user.message_id)
            .await?;
        let model_input = render_stored_user_message(&prompt);
        let _ = self
            .start_turn_for_stored_message(session_id, &last_user.message_id, model_input)
            .await?;
        self.store.get_session_detail(session_id).await
    }

    pub async fn compact(&self, session_id: &str) -> RuntimeResult<SessionDetail> {
        let gate = self.session_gate(session_id).await;
        let _guard = gate.lock_owned().await;
        self.compact_locked(session_id, None).await
    }

    async fn compact_locked(
        &self,
        session_id: &str,
        exclude_message_id: Option<&str>,
    ) -> RuntimeResult<SessionDetail> {
        let started_at = self.store.begin_compaction(session_id).await?;
        let mut flag_guard = CompactionFlagGuard::new(self.store.clone(), session_id, started_at);
        if let Err(error) = self
            .emit(
                session_id,
                "compaction",
                "main",
                None,
                SessionEventKind::CompactionStarted,
            )
            .await
        {
            let _ = self
                .store
                .clear_compaction_if_started(session_id, started_at)
                .await;
            flag_guard.disarm();
            return Err(error);
        }
        let result = self
            .compact_started(session_id, exclude_message_id, started_at)
            .await;
        match &result {
            Ok(_) => {
                let _ = self
                    .emit(
                        session_id,
                        "compaction",
                        "main",
                        None,
                        SessionEventKind::CompactionFinished { success: true },
                    )
                    .await;
            }
            Err(_) => {
                if let Err(error) = self
                    .store
                    .clear_compaction_if_started(session_id, started_at)
                    .await
                {
                    warn!(%error, %session_id, "failed to clear compaction state");
                }
                let _ = self
                    .emit(
                        session_id,
                        "compaction",
                        "main",
                        None,
                        SessionEventKind::CompactionFinished { success: false },
                    )
                    .await;
            }
        }
        flag_guard.disarm();
        result
    }

    async fn compact_started(
        &self,
        session_id: &str,
        exclude_message_id: Option<&str>,
        started_at: chrono::DateTime<chrono::Utc>,
    ) -> RuntimeResult<SessionDetail> {
        let session = self.store.get_session_summary(session_id).await?;
        if session.active_run_status == ActiveRunStatus::Running
            || self.store.latest_running_run(session_id).await?.is_some()
        {
            return Err(RuntimeError::CompactionConflict(
                "session has an active run; wait for it to finish before compacting".into(),
            ));
        }

        let memory = self.store.memory().await?;
        let history = memory
            .history_for_compaction(session_id, exclude_message_id)
            .await?;
        if history.is_empty() {
            let prompt = self
                .kb
                .system_prompt_for(session.kb_branch.as_deref())
                .await?;
            let version = self.kb.current_version().await?;
            self.store
                .set_pinned_prompt(session_id, prompt.as_str(), version)
                .await?;
            self.store
                .clear_compaction_if_started(session_id, started_at)
                .await?;
            return self.store.get_session_detail(session_id).await;
        }

        let old_prompt = match self.store.pinned_prompt(session_id).await? {
            (Some(prompt), _) => prompt,
            (None, _) => self
                .kb
                .system_prompt_for(session.kb_branch.as_deref())
                .await?
                .as_str()
                .to_string(),
        };
        let agent_definition = self
            .kb
            .get_default_agent_on(session.kb_branch.as_deref())
            .await?;
        let model_route = agent_definition
            .as_ref()
            .map(|agent| agent.model.clone())
            .unwrap_or_else(|| self.config.default_model.clone());
        let model = self.models.build_rig_model(&model_route).await?;
        let agent = rig::agent::AgentBuilder::new(model)
            .preamble(old_prompt)
            .default_max_turns(1)
            .build();
        let documents = vec![
            rig::completion::Document {
                id: "quire-compaction-mode".into(),
                text: COMPACTION_MODE_MESSAGE.into(),
                additional_props: Default::default(),
            },
            rig::completion::Document {
                id: "quire-report-state".into(),
                text: format!("doc_id: {}", session.doc_id.as_deref().unwrap_or("none")),
                additional_props: Default::default(),
            },
        ];
        let response = tokio::time::timeout(
            Duration::from_secs(COMPACTION_TIMEOUT_SECS),
            agent
                .prompt_typed::<crate::compaction::CompactionHandoff>(COMPACTION_INSTRUCTION)
                .history(history)
                .documents(documents)
                .max_turns(1)
                .retries(1),
        )
        .await
        .map_err(|_| RuntimeError::Internal("compaction summary timed out".into()))?
        .map_err(|error| {
            RuntimeError::Internal(format!("structured compaction failed: {error}"))
        })?;
        let summary = response.output.to_markdown();

        let latest_prompt = self
            .kb
            .system_prompt_for(session.kb_branch.as_deref())
            .await?;
        let latest_version = self.kb.current_version().await?;
        let resolved_model = self.models.resolve(&model_route).await.ok();
        let compaction_cost = resolved_model.as_ref().and_then(|resolved| {
            resolved.calculate_cost_rmb(
                response.usage.input_tokens.unwrap_or(0),
                response.usage.output_tokens.unwrap_or(0),
                response.usage.cached_input_tokens.unwrap_or(0),
            )
        });
        let cost_rmb = match (session.context_usage.cost_rmb, compaction_cost) {
            (Some(prev), Some(delta)) => Some(prev + delta),
            (Some(prev), None) => Some(prev),
            (None, Some(delta)) => Some(delta),
            (None, None) => None,
        };
        let estimated_tokens = estimate_context_tokens(&summary);
        let context_usage = ContextUsage {
            input_tokens: estimated_tokens,
            output_tokens: 0,
            cached_tokens: 0,
            context_used: estimated_tokens,
            context_limit: session.context_usage.context_limit,
            cost_rmb,
            output_price_rmb: resolved_model
                .as_ref()
                .and_then(|model| model.cost_output)
                .or(session.context_usage.output_price_rmb),
        };
        self.store
            .apply_compaction(&CompactionWrite {
                session_id,
                summary: &summary,
                exclude_message_id,
                pinned_system_prompt: latest_prompt.as_str(),
                kb_version: latest_version,
                context_usage: &context_usage,
            })
            .await?;
        self.store.get_session_detail(session_id).await
    }

    async fn maybe_auto_compact(&self, session_id: &str, exclude_message_id: Option<&str>) {
        let Ok(session) = self.store.get_session_summary(session_id).await else {
            return;
        };
        if session.active_run_status != ActiveRunStatus::Idle || session.compacting {
            return;
        }
        let usage = &session.context_usage;
        if usage.context_limit == 0
            || usage.context_used.saturating_mul(100) < usage.context_limit.saturating_mul(80)
        {
            return;
        }
        if let Err(error) = self.compact_locked(session_id, exclude_message_id).await {
            warn!(%error, %session_id, "automatic context compaction failed; continuing turn");
        }
    }

    pub async fn get_tool_call(
        &self,
        session_id: &str,
        tool_call_id: &str,
    ) -> RuntimeResult<ToolCallDetail> {
        self.store.get_tool_call(session_id, tool_call_id).await
    }

    pub(crate) async fn ensure_pinned_prompt(
        &self,
        session_id: &str,
    ) -> RuntimeResult<(String, u64)> {
        let (pinned, synced) = self.store.pinned_prompt(session_id).await?;
        if let Some(pinned) = pinned {
            return Ok((pinned, synced));
        }
        let session = self.store.get_session_summary(session_id).await?;
        let prompt = self
            .kb
            .system_prompt_for(session.kb_branch.as_deref())
            .await?;
        let version = self.kb.current_version().await?;
        self.store
            .set_pinned_prompt(session_id, prompt.as_str(), version)
            .await?;
        Ok((prompt.as_str().to_string(), version))
    }

    async fn prepare_turn(&self, session_id: &str) -> RuntimeResult<(String, CancellationToken)> {
        self.store.get_session_summary(session_id).await?;
        if self.store.latest_running_run(session_id).await?.is_some() {
            return Err(RuntimeError::InvalidRequest(format!(
                "session `{session_id}` already has an active run; queue the message instead"
            )));
        }
        let run = self
            .store
            .create_run(session_id, None, "main", &self.config.default_model)
            .await?;
        let token = self.runs.register(&run.run_id).await;
        Ok((run.run_id, token))
    }

    pub(crate) fn launch_turn(
        &self,
        session_id: &str,
        run_id: &str,
        content: String,
        current_message_id: Option<String>,
        token: CancellationToken,
    ) {
        let service = self.clone();
        let run_id = run_id.to_string();
        let session_id = session_id.to_string();
        tokio::spawn(async move {
            if let Err(error) = service
                .run_turn(
                    &session_id,
                    &run_id,
                    &content,
                    current_message_id.as_deref(),
                    token,
                )
                .await
            {
                warn!(%error, %session_id, run_id, "agent turn failed");
            }
        });
    }

    pub(crate) fn launch_resumed_turn(
        &self,
        session_id: String,
        run_id: String,
        token: CancellationToken,
    ) {
        let service = self.clone();
        tokio::spawn(async move {
            let _run_guard = RunTurnGuard::new(&run_id, token.clone(), service.runs.clone());
            let result = service
                .execute_resumed_turn(&session_id, &run_id, &token)
                .await;
            if let Err(error) = service
                .finish_turn_outcome(&session_id, &run_id, &token, result)
                .await
            {
                warn!(%error, %session_id, %run_id, "resumed agent turn failed");
            }
        });
    }

    pub(crate) async fn spawn_turn(
        &self,
        session_id: &str,
        content: String,
    ) -> RuntimeResult<String> {
        self.maybe_auto_compact(session_id, None).await;
        let request = SendMessageRequest {
            client_msg_id: short_id("internal"),
            content: content.clone(),
            comment_refs: Vec::new(),
            attachment_ids: Vec::new(),
        };
        self.store
            .memory()
            .await?
            .append_prompt(session_id, &request, false)
            .await?;
        let (run_id, token) = self.prepare_turn(session_id).await?;
        self.launch_turn(session_id, &run_id, content, None, token);
        Ok(run_id)
    }

    pub(crate) async fn start_turn_for_stored_message(
        &self,
        session_id: &str,
        message_id: &str,
        content: String,
    ) -> RuntimeResult<String> {
        self.maybe_auto_compact(session_id, Some(message_id)).await;
        let (run_id, token) = self.prepare_turn(session_id).await?;
        self.launch_turn(
            session_id,
            &run_id,
            content,
            Some(message_id.to_string()),
            token,
        );
        Ok(run_id)
    }

    pub(crate) async fn spawn_turn_for_message(
        &self,
        session_id: &str,
        message_id: &str,
        content: String,
    ) -> RuntimeResult<String> {
        self.maybe_auto_compact(session_id, Some(message_id)).await;
        let (run_id, token) = self.prepare_turn(session_id).await?;
        if let Err(error) = self
            .emit(
                session_id,
                &run_id,
                "main",
                None,
                SessionEventKind::UserMessageAdded {
                    message_id: message_id.to_string(),
                },
            )
            .await
        {
            warn!(%error, %session_id, %run_id, "failed to broadcast user message");
        }
        self.launch_turn(
            session_id,
            &run_id,
            content,
            Some(message_id.to_string()),
            token,
        );
        Ok(run_id)
    }

    async fn run_turn(
        &self,
        session_id: &str,
        run_id: &str,
        content: &str,
        current_message_id: Option<&str>,
        token: CancellationToken,
    ) -> RuntimeResult<()> {
        let _run_guard = RunTurnGuard::new(run_id, token.clone(), self.runs.clone());
        let result = self
            .execute_turn(session_id, run_id, content, current_message_id, &token)
            .await;
        self.finish_turn_outcome(session_id, run_id, &token, result)
            .await
    }

    async fn finish_turn_outcome(
        &self,
        session_id: &str,
        run_id: &str,
        token: &CancellationToken,
        result: RuntimeResult<()>,
    ) -> RuntimeResult<()> {
        match result {
            Ok(()) => {
                if token.is_cancelled() {
                    return Ok(());
                }
                let session = self.store.get_session_summary(session_id).await?;
                if session.active_run_status != ActiveRunStatus::AwaitingInput {
                    self.queue.notify_one();
                }
                Ok(())
            }
            Err(error) => {
                let was_cancelled = token.is_cancelled();
                // Stop the producer before reporting failure: a persistence or mapping error
                // must not leave the agent executing tools after the run is marked as failed.
                token.cancel();
                if was_cancelled {
                    self.finish_cancelled_run(run_id).await?;
                } else {
                    let message = error.to_string();
                    let _ = self
                        .store
                        .finish_run_with_event(
                            run_id,
                            "failed",
                            Some(&message),
                            SessionEventKind::TurnFailed {
                                error: message.clone(),
                                recoverable: true,
                            },
                        )
                        .await;
                }
                Err(error)
            }
        }
    }

    /// Finish a user-cancelled run after the turn has flushed its deltas.
    /// A concurrent finisher simply makes this a no-op.
    pub(crate) async fn finish_cancelled_run(&self, run_id: &str) -> RuntimeResult<()> {
        let _ = self
            .store
            .finish_run_with_event(
                run_id,
                "cancelled",
                Some("interrupted by user"),
                SessionEventKind::TurnFailed {
                    error: "interrupted by user".into(),
                    recoverable: true,
                },
            )
            .await?;
        Ok(())
    }
}
