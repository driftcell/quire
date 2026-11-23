use super::*;

#[tool_router(router = sessions_router, vis = "pub(crate)")]
impl AdminMcpServer {
    /// List runtime sessions ordered by pinned state and most recent activity (`updated_at`), including bound `doc_id`, `kb_branch`, `active_run_status`, and cumulative `context_usage` (`cost_rmb`).
    #[tool(name = "sessions_list")]
    pub async fn sessions_list(
        &self,
        Parameters(request): Parameters<SessionsListParams>,
    ) -> Result<Json<SessionsListReceipt>, ErrorData> {
        let sessions = self
            .service
            .list_sessions(request.limit.unwrap_or(50), request.offset.unwrap_or(0))
            .await
            .map_err(admin_error)?;
        let next_hint = if sessions.is_empty() {
            "No sessions yet. Create one through POST /api/sessions with an optional kb_branch."
                .to_string()
        } else {
            "Call sessions_inspect with a session_id for messages, latest run and context usage."
                .to_string()
        };
        Ok(Json(SessionsListReceipt {
            sessions,
            next_hint,
        }))
    }

    /// Inspect the full state of a session (`messages`, `attachments`, unresolved `pending_question`, `pending_queue`, `latest_run`, and `context_usage`).
    ///
    /// Use this to diagnose why a session is `running`, `awaiting_input`, `interrupted`, or `failed`; to inspect per-turn thinking/tool/subagent timelines, call `sessions_traces`.
    #[tool(name = "sessions_inspect")]
    pub async fn sessions_inspect(
        &self,
        Parameters(request): Parameters<SessionsInspectParams>,
    ) -> Result<Json<SessionsInspectReceipt>, ErrorData> {
        let inspection = self
            .service
            .inspect_session(&request.session_id)
            .await
            .map_err(admin_error)?;
        let context_usage = inspection.detail.summary.context_usage.clone();
        let latest_run = inspection.latest_run.map(RunSummaryView::from_record);
        let next_hint = match latest_run.as_ref().map(|run| run.status.as_str()) {
            Some("running") => "Run is active. Watch Session-SSE or call sessions_inspect again."
                .to_string(),
            Some("failed") => {
                "Latest run failed. Read the run error, fix the branch/model configuration, then retry."
                    .to_string()
            }
            Some(_) => "Run completed. Compare branch content with kb_diff_branch before promotion."
                .to_string(),
            None => "Session has no runs yet. POST a message to start the first turn.".to_string(),
        };
        Ok(Json(SessionsInspectReceipt {
            session: inspection.detail,
            latest_run,
            context_usage,
            next_hint,
        }))
    }

    /// List ordered execution trace summaries (`thinking`, `execution` tool calls and subagent runs, and `text` segments) for all runs in a session.
    ///
    /// Tool inputs and outputs in `RunTraceToolItem` are 240-char previews (`truncated: true` when clipped); pass any `tool_call_id` to `sessions_get_tool_call` to inspect the full untruncated JSON input, output, and error payload.
    #[tool(name = "sessions_traces")]
    pub async fn sessions_traces(
        &self,
        Parameters(request): Parameters<SessionsInspectParams>,
    ) -> Result<Json<SessionsTracesReceipt>, ErrorData> {
        let traces = self
            .service
            .list_run_traces(&request.session_id)
            .await
            .map_err(admin_error)?;
        let next_hint = if traces.is_empty() {
            "No run traces recorded for this session yet.".to_string()
        } else {
            "Call sessions_get_tool_call with a tool_call_id to inspect full input/output."
                .to_string()
        };
        Ok(Json(SessionsTracesReceipt {
            session_id: request.session_id,
            traces,
            next_hint,
        }))
    }

    /// Fetch the complete, untruncated `input` JSON, `output` JSON, `error`, and `duration_ms` for a specific `tool_call_id` in a session.
    ///
    /// Use this as the second step after `sessions_traces` identifies a failed, slow, or truncated tool call (including subagent tool calls within the same `session_id`).
    #[tool(name = "sessions_get_tool_call")]
    pub async fn sessions_get_tool_call(
        &self,
        Parameters(request): Parameters<SessionsToolCallParams>,
    ) -> Result<Json<SessionsToolCallReceipt>, ErrorData> {
        let tool_call = self
            .service
            .get_tool_call(&request.session_id, &request.tool_call_id)
            .await
            .map_err(admin_error)?;
        let next_hint = format!(
            "Tool `{}` finished with status `{}`.",
            tool_call.tool_name, tool_call.status
        );
        Ok(Json(SessionsToolCallReceipt {
            tool_call,
            next_hint,
        }))
    }

    /// Update administrative metadata on a session (`title`, `status` between `active` and `archived`, or `pinned`).
    ///
    /// Does not interrupt or modify running turns; use `sessions_action` for run lifecycle operations (`interrupt`, `retry`, `compact`, `delete`).
    #[tool(name = "sessions_update")]
    pub async fn sessions_update(
        &self,
        Parameters(request): Parameters<SessionsUpdateParams>,
    ) -> Result<Json<SessionsUpdateReceipt>, ErrorData> {
        let session = self
            .service
            .update_session(
                &request.session_id,
                SessionPatch {
                    title: request.title.as_deref(),
                    status: request.status,
                    pinned: request.pinned,
                },
            )
            .await
            .map_err(admin_error)?;
        let next_hint = format!("Session `{}` updated.", session.session_id);
        Ok(Json(SessionsUpdateReceipt { session, next_hint }))
    }

    /// Execute a lifecycle control action (`interrupt`, `retry`, `compact`, or `delete`) on a session.
    ///
    /// Action semantics & preconditions:
    /// - `interrupt`: cancels the active run (or specific `run_id`) and transitions the session out of `running`.
    /// - `retry`: re-runs the latest `failed` or `interrupted` turn using the current knowledge-base and model configuration.
    /// - `compact`: summarizes conversation history into a compaction checkpoint and refreshes the pinned system prompt to the latest KB version (requires the session to be idle).
    /// - `delete`: permanently deletes the session, its messages, runs, tool traces, and attachments (requires no active run; does not delete the bound Quire report document).
    #[tool(name = "sessions_action")]
    pub async fn sessions_action(
        &self,
        Parameters(request): Parameters<SessionsActionParams>,
    ) -> Result<Json<SessionsActionReceipt>, ErrorData> {
        let session = match request.action {
            SessionAdminAction::Interrupt => Some(
                self.service
                    .interrupt(&request.session_id, request.run_id.as_deref())
                    .await
                    .map_err(admin_error)?,
            ),
            SessionAdminAction::Retry => Some(
                self.service
                    .retry(&request.session_id)
                    .await
                    .map_err(admin_error)?,
            ),
            SessionAdminAction::Compact => Some(
                self.service
                    .compact(&request.session_id)
                    .await
                    .map_err(admin_error)?,
            ),
            SessionAdminAction::Delete => {
                self.service
                    .delete_session(&request.session_id)
                    .await
                    .map_err(admin_error)?;
                None
            }
        };
        let next_hint = match request.action {
            SessionAdminAction::Interrupt => {
                "Interrupt sent. Call sessions_inspect to verify idle/failed state.".to_string()
            }
            SessionAdminAction::Retry => {
                "Retry started. Watch Session-SSE or call sessions_inspect.".to_string()
            }
            SessionAdminAction::Compact => {
                "Pinned system prompt compacted to latest KB version.".to_string()
            }
            SessionAdminAction::Delete => {
                format!("Session `{}` deleted.", request.session_id)
            }
        };
        Ok(Json(SessionsActionReceipt {
            session_id: request.session_id,
            action: request.action,
            session,
            next_hint,
        }))
    }
}
