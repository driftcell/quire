use super::*;

impl ToolLifecycleHook {
    pub(crate) fn new(
        execution: AgentExecution,
        scope: StreamEventSender,
        kb: KbService,
        previews: Arc<Mutex<PreviewState>>,
        state: SharedTurnState,
    ) -> Self {
        Self {
            execution,
            scope,
            kb: Some(kb),
            previews,
            state,
        }
    }

    #[cfg(test)]
    pub(super) fn for_test(
        execution: AgentExecution,
        scope: StreamEventSender,
        previews: Arc<Mutex<PreviewState>>,
        state: SharedTurnState,
    ) -> Self {
        Self {
            execution,
            scope,
            kb: None,
            previews,
            state,
        }
    }

    fn arguments(raw: Option<&str>) -> Value {
        raw.and_then(|raw| serde_json::from_str(raw).ok())
            .unwrap_or(Value::Null)
    }

    async fn send_started(&self, tool_call_id: String, tool_name: &str, arguments: Value) {
        let (input_preview, truncated) = preview_json(&arguments);
        self.state
            .lock()
            .await
            .started
            .insert(tool_call_id.clone(), Instant::now());
        self.scope
            .send(ModelStreamEvent::ToolCallStarted {
                tool_call_id,
                tool_name: tool_name.to_string(),
                input_preview,
                truncated,
                input: arguments,
            })
            .await;
    }

    async fn bind_created_report(&self, arguments: &Value, output: &Value) {
        let Some(doc_id) = output.get("doc_id").and_then(Value::as_str) else {
            return;
        };
        let title = output
            .get("title")
            .and_then(Value::as_str)
            .or_else(|| arguments.get("title").and_then(Value::as_str))
            .unwrap_or("未命名报告")
            .to_string();
        if let Some(kb) = self.kb.as_ref()
            && let Err(error) = kb
                .store()
                .bind_doc(&self.execution.session_id, doc_id)
                .await
        {
            tracing::warn!(%error, session_id = %self.execution.session_id, "failed to bind lazily created report");
            return;
        }
        update_doc_id(&self.execution.doc_id, Some(doc_id.to_string()));
        self.scope
            .send(ModelStreamEvent::DocLinked {
                doc_id: doc_id.to_string(),
                title,
            })
            .await;
    }
}

impl AgentHook for ToolLifecycleHook {
    fn observes(&self, kind: StepEventKind) -> bool {
        matches!(
            kind,
            StepEventKind::CompletionCall
                | StepEventKind::ModelTurnFinished
                | StepEventKind::ToolCallDelta
                | StepEventKind::ToolDispatch
                | StepEventKind::InvalidToolCall
        )
    }

    async fn on_completion_call(
        &self,
        _ctx: &HookContext,
        _event: CompletionCallEvent<'_>,
    ) -> CompletionCallAction {
        self.previews.lock().await.abort_all().await;
        CompletionCallAction::continue_run()
    }

    async fn on_model_turn_finished(
        &self,
        _ctx: &HookContext,
        event: ModelTurnFinished<'_>,
    ) -> ModelTurnAction {
        let has_question = event.content.iter().any(|content| {
            matches!(content, AssistantContent::ToolCall(call) if call.function.name.as_str() == "ask_user")
        });
        self.state.lock().await.turn_has_ask_user = has_question;
        if has_question {
            self.previews.lock().await.abort_all().await;
        }
        ModelTurnAction::continue_run()
    }

    async fn on_tool_call_delta(
        &self,
        ctx: &HookContext,
        event: ToolCallDelta<'_>,
    ) -> ObservationAction {
        self.previews
            .lock()
            .await
            .on_tool_delta(ctx.turn(), event.part.index(), event.tool_name, event.delta)
            .await;
        ObservationAction::continue_run()
    }

    async fn on_dispatch(&self, ctx: &HookContext, event: DispatchEvent<'_>) -> DispatchAction {
        let Some(tool_name) = event.tool_name() else {
            return DispatchAction::proceed();
        };
        let has_question = self.state.lock().await.turn_has_ask_user;
        let Some(call_id) = event.call_id else {
            return if has_question && tool_name != "ask_user" {
                DispatchAction::skip("skipped because ask_user ends this turn")
            } else {
                DispatchAction::proceed()
            };
        };
        let tool_call_id = format!("{}:{}", self.execution.run_id, call_id.wire());
        let mut arguments = Self::arguments(event.tool_args());

        if has_question && tool_name != "ask_user" {
            self.previews.lock().await.abort_call(&call_id.wire()).await;
            self.send_started(tool_call_id, tool_name, arguments).await;
            let output = serde_json::to_string(&SkippedToolOutput {
                kind: "skipped",
                reason: "ask_user_sibling",
                message: "not executed because ask_user ends this turn; replan after the structured answer",
            })
            .unwrap_or_else(|error| {
                tracing::error!(%error, "failed to serialize skipped tool output");
                "not executed because ask_user ends this turn".to_string()
            });
            return DispatchAction::skip(output.to_string());
        }

        let mut action = DispatchAction::proceed();
        if tool_name == "apply_operations" {
            let mut previews = self.previews.lock().await;
            previews
                .finish_tool_call(ctx.turn(), &call_id.wire(), tool_name, &arguments)
                .await;
            if let Some(ids) = previews.take_preview_ids(&call_id.wire()) {
                inject_preview_ids(&mut arguments, &ids);
                action = DispatchAction::rewrite_tool_args(event.kind, arguments.clone());
            }
        }
        self.send_started(tool_call_id, tool_name, arguments).await;
        action
    }

    async fn on_outcome(&self, _ctx: &HookContext, event: OutcomeEvent<'_>) -> OutcomeAction {
        let (Some(tool_name), Some(call_id)) = (event.tool_name(), event.call_id) else {
            return OutcomeAction::proceed();
        };
        let wire_call_id = call_id.wire();
        let tool_call_id = format!("{}:{}", self.execution.run_id, wire_call_id);
        let arguments = Self::arguments(event.tool_args());
        let duration_ms = self
            .state
            .lock()
            .await
            .started
            .remove(&tool_call_id)
            .and_then(|started| u64::try_from(started.elapsed().as_millis()).ok())
            .unwrap_or(0);
        let (mut status, mut output, error) = if let Some(result) = event.tool_result() {
            let status = if result.is_skipped() {
                "skipped"
            } else if result.is_error() || result.is_refused() {
                "failed"
            } else {
                "completed"
            };
            let error = result
                .error()
                .map(ToString::to_string)
                .or_else(|| result.refusal().map(ToString::to_string));
            let output = if result.is_skipped() {
                result
                    .output()
                    .as_text()
                    .and_then(|text| serde_json::from_str(text).ok())
                    .unwrap_or_else(|| tool_output_value(result.output()))
            } else {
                tool_output_value(result.output())
            };
            (status, output, error)
        } else {
            let message = event
                .outcome
                .as_ref()
                .err()
                .map(|report| report.message.clone())
                .unwrap_or_else(|| "tool dispatch failed".into());
            (
                "failed",
                serde_json::to_value(ToolErrorOutput { error: &message }).unwrap_or_else(|error| {
                    tracing::error!(%error, "failed to serialize tool error output");
                    Value::Null
                }),
                Some(message),
            )
        };

        if tool_name == "apply_operations" {
            let mut previews = self.previews.lock().await;
            if status == "completed" {
                previews.confirm_call(&wire_call_id);
            } else {
                previews.abort_call(&wire_call_id).await;
            }
        }

        let requested = (status == "completed" && tool_name == "ask_user")
            .then(|| {
                event
                    .tool_context()
                    .and_then(|context| context.result::<AskUserRequestedMeta>().ok().flatten())
            })
            .flatten();
        if self.execution.cancellation.is_cancelled() {
            terminate(&self.state, RunTermination::Cancelled).await;
        } else if let Some(requested) = requested {
            self.scope
                .send(ModelStreamEvent::QuestionnaireRequested {
                    tool_call_id: Some(tool_call_id.clone()),
                    questions: requested.questions,
                })
                .await;
            status = "awaiting_input";
            output = serde_json::to_value(AwaitingInputOutput {
                status: "awaiting_input",
            })
            .unwrap_or_else(|error| {
                tracing::error!(%error, "failed to serialize awaiting-input output");
                Value::Null
            });
            terminate(&self.state, RunTermination::AskUser).await;
        } else if status == "completed" && tool_name == "create_report" {
            self.bind_created_report(&arguments, &output).await;
        }

        let (output_preview, truncated) = preview_json(&output);
        self.scope
            .send(ModelStreamEvent::ToolCallFinished {
                tool_call_id,
                tool_name: tool_name.to_string(),
                status: status.to_string(),
                output_preview,
                truncated,
                duration_ms,
                input: arguments,
                output,
                error,
            })
            .await;

        if self.state.lock().await.termination == RunTermination::Cancelled {
            OutcomeAction::stop(CANCELLED_REASON)
        } else {
            OutcomeAction::proceed()
        }
    }

    async fn on_invalid_tool_call(
        &self,
        _ctx: &HookContext,
        event: &InvalidToolCallContext,
    ) -> Option<InvalidToolCallAction> {
        match &event.reason {
            InvalidToolCallReason::MalformedArguments { error } => {
                Some(InvalidToolCallAction::retry(format!(
                    "Tool `{}` received malformed arguments: {error}. Return one JSON object matching the advertised schema.",
                    event.tool_name
                )))
            }
            InvalidToolCallReason::UnknownTool | InvalidToolCallReason::DisallowedByToolChoice => {
                Some(InvalidToolCallAction::retry(format!(
                    "Tool `{}` is not available for this turn. Use only these advertised tools: {:?}.",
                    event.tool_name, event.available_tools
                )))
            }
            _ => None,
        }
    }
}
