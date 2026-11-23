use super::*;

#[tool_router(router = sessions_router, vis = "pub(crate)")]
impl AdminMcpServer {
    #[tool(
        name = "sessions_list",
        description = quire_protocol::tool_description!("sessions_list")
    )]
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

    #[tool(
        name = "sessions_inspect",
        description = quire_protocol::tool_description!("sessions_inspect")
    )]
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

    #[tool(
        name = "sessions_traces",
        description = quire_protocol::tool_description!("sessions_traces")
    )]
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

    #[tool(
        name = "sessions_get_tool_call",
        description = quire_protocol::tool_description!("sessions_get_tool_call")
    )]
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

    #[tool(
        name = "sessions_update",
        description = quire_protocol::tool_description!("sessions_update")
    )]
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

    #[tool(
        name = "sessions_action",
        description = quire_protocol::tool_description!("sessions_action")
    )]
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
