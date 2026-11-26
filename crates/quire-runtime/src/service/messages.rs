use super::*;

impl RuntimeService {
    pub async fn post_message(
        &self,
        session_id: &str,
        request: &SendMessageRequest,
    ) -> RuntimeResult<PostMessageOutcome> {
        self.post_message_with_source(session_id, request, PendingQueueSource::User, None)
            .await
    }

    /// Submit an auto-generated comment review message. Queued instances keep
    /// their `Comment` source so consecutive comments can be merged and queue
    /// deletion can remove the still-open server thread.
    pub async fn post_comment_message(
        &self,
        session_id: &str,
        comment_ref: SessionCommentRef,
    ) -> RuntimeResult<PostMessageOutcome> {
        let thread_id = comment_ref.thread_id.clone();
        let client_msg_id = format!("comment:{thread_id}");
        self.post_comment_message_with_id(session_id, &thread_id, &client_msg_id, comment_ref)
            .await
    }

    /// Re-submit an addressed comment thread after a human follow-up reply.
    /// The reply message ID keeps retries idempotent while allowing one run per
    /// new instruction.
    pub async fn post_comment_reply_message(
        &self,
        session_id: &str,
        reply_message_id: &str,
        comment_ref: SessionCommentRef,
    ) -> RuntimeResult<PostMessageOutcome> {
        let thread_id = comment_ref.thread_id.clone();
        let client_msg_id = format!("comment:{thread_id}:{reply_message_id}");
        self.post_comment_message_with_id(session_id, &thread_id, &client_msg_id, comment_ref)
            .await
    }

    async fn post_comment_message_with_id(
        &self,
        session_id: &str,
        thread_id: &str,
        client_msg_id: &str,
        comment_ref: SessionCommentRef,
    ) -> RuntimeResult<PostMessageOutcome> {
        if comment_ref.thread_id != thread_id {
            return Err(RuntimeError::InvalidRequest(
                "comment reference thread_id must match the comment queue thread".into(),
            ));
        }
        let request = SendMessageRequest {
            client_msg_id: client_msg_id.to_string(),
            content: String::new(),
            comment_refs: vec![comment_ref],
            attachment_ids: Vec::new(),
        };
        self.post_message_with_source(
            session_id,
            &request,
            PendingQueueSource::Comment,
            Some(thread_id),
        )
        .await
    }

    async fn post_message_with_source(
        &self,
        session_id: &str,
        request: &SendMessageRequest,
        source: PendingQueueSource,
        thread_id: Option<&str>,
    ) -> RuntimeResult<PostMessageOutcome> {
        let gate = self.session_gate(session_id).await;
        let _guard = gate.lock_owned().await;
        let session = self.store.get_session_summary(session_id).await?;
        if session.status == SessionStatus::Archived {
            return Err(RuntimeError::InvalidRequest(
                "archived sessions cannot receive new messages".into(),
            ));
        }
        let awaiting_input = session.active_run_status == ActiveRunStatus::AwaitingInput;
        let active = !awaiting_input
            && (session.active_run_status == ActiveRunStatus::Running
                || self.store.latest_running_run(session_id).await?.is_some());
        if let Some(message) = self
            .store
            .find_message_by_client_id(session_id, &request.client_msg_id)
            .await?
        {
            if !active
                && !awaiting_input
                && !self
                    .store
                    .has_run_for_message(session_id, &message.message_id)
                    .await?
            {
                let message_id = message.message_id.clone();
                let model_input = render_stored_user_message(&message);
                let run_id = self
                    .spawn_turn_for_message(session_id, &message_id, model_input)
                    .await?;
                return Ok(PostMessageOutcome::Started { message, run_id });
            }
            return Ok(PostMessageOutcome::Replayed { message });
        }
        let queue_non_empty =
            !awaiting_input && !self.store.list_pending_queue(session_id).await?.is_empty();
        if source == PendingQueueSource::Comment
            && let Some(thread_id) = thread_id
            && let Some(existing) = self
                .store
                .pending_comment_item_for_thread(session_id, thread_id)
                .await?
        {
            let item = self
                .store
                .update_pending_message(
                    session_id,
                    &existing.queue_id,
                    &request.content,
                    &request.comment_refs,
                )
                .await?;
            self.emit_queue_update(session_id).await?;
            if !active && !awaiting_input {
                self.drain_pending_locked(session_id, true).await?;
            }
            return Ok(PostMessageOutcome::Queued { item });
        }
        if active || queue_non_empty {
            let item = self
                .store
                .enqueue_pending_message(EnqueuePendingMessage {
                    session_id,
                    client_msg_id: &request.client_msg_id,
                    content: &request.content,
                    comment_refs: &request.comment_refs,
                    source,
                    thread_id,
                    attachment_ids: &request.attachment_ids,
                })
                .await?;
            self.emit_queue_update(session_id).await?;
            if !active {
                self.drain_pending_locked(session_id, true).await?;
            }
            return Ok(PostMessageOutcome::Queued { item });
        }
        let dismissed = self.store.dismiss_pending_questions(session_id).await?;
        if !dismissed.is_empty() {
            let memory = self.store.memory().await?;
            for question in &dismissed {
                let payload = AskUserDismissalPayload {
                    kind: "ask_user_response",
                    question_id: &question.question_id,
                    tool_call_id: &question.tool_call_id,
                    status: "dismissed",
                    reason: "superseded_by_user_message",
                };
                memory
                    .append_internal_prompt(
                        session_id,
                        format!("question-dismissal:{}", question.question_id),
                        serde_json::to_string(&payload)?,
                    )
                    .await?;
            }
        }
        let (message, inserted) = self.store.insert_user_message(session_id, request).await?;
        if !inserted
            && self
                .store
                .has_run_for_message(session_id, &message.message_id)
                .await?
        {
            return Ok(PostMessageOutcome::Replayed { message });
        }
        self.clear_dismissed_questions(session_id, dismissed, "superseded_by_user_message")
            .await?;
        let message_id = message.message_id.clone();
        let model_input = render_stored_user_message(&message);
        let run_id = self
            .spawn_turn_for_message(session_id, &message_id, model_input)
            .await?;
        Ok(PostMessageOutcome::Started { message, run_id })
    }

    pub async fn list_pending_queue(
        &self,
        session_id: &str,
    ) -> RuntimeResult<Vec<PendingQueueItem>> {
        self.store.list_pending_queue(session_id).await
    }

    pub async fn update_pending_message(
        &self,
        session_id: &str,
        queue_id: &str,
        content: &str,
        comment_refs: &[SessionCommentRef],
    ) -> RuntimeResult<PendingQueueItem> {
        let gate = self.session_gate(session_id).await;
        let _guard = gate.lock_owned().await;
        let item = self
            .store
            .update_pending_message(session_id, queue_id, content, comment_refs)
            .await?;
        self.emit_queue_update(session_id).await?;
        Ok(item)
    }

    pub async fn delete_pending_message(
        &self,
        session_id: &str,
        queue_id: &str,
    ) -> RuntimeResult<bool> {
        let gate = self.session_gate(session_id).await;
        let _guard = gate.lock_owned().await;
        let item = self
            .store
            .get_pending_queue_item(session_id, queue_id)
            .await?;
        let deleted = self
            .store
            .delete_pending_message(session_id, queue_id)
            .await?;
        if deleted {
            if let Some(item) = item
                && item.source == PendingQueueSource::Comment
                && let Some(thread_id) = item.thread_id.as_deref()
                && let Err(error) = self.delete_server_comment(session_id, thread_id).await
            {
                warn!(
                    %error,
                    %session_id,
                    %thread_id,
                    "failed to delete the comment linked to a removed queue item"
                );
            }
            self.emit_queue_update(session_id).await?;
        }
        Ok(deleted)
    }

    pub async fn remove_pending_comment_threads(
        &self,
        session_id: &str,
        thread_id: &str,
    ) -> RuntimeResult<u64> {
        let gate = self.session_gate(session_id).await;
        let _guard = gate.lock_owned().await;
        let removed = self
            .store
            .remove_pending_comment_thread(session_id, thread_id)
            .await?;
        if removed > 0 {
            self.emit_queue_update(session_id).await?;
        }
        Ok(removed)
    }

    pub fn report_app_state(&self) -> quire_server::AppState {
        let bridge = std::sync::Arc::new(RuntimeExternalHarnessBridge::new(self.store.clone()));
        quire_server::AppState::new(
            self.store.report_store().clone(),
            self.config.to_report_server_config(),
        )
        .with_harness_bridge(bridge)
    }

    async fn delete_server_comment(&self, session_id: &str, thread_id: &str) -> RuntimeResult<()> {
        let session = self.store.get_session_summary(session_id).await?;
        let raw_doc_id = session
            .doc_id
            .filter(|doc_id| !doc_id.trim().is_empty())
            .or_else(|| {
                session
                    .linked_doc_id
                    .filter(|doc_id| !doc_id.trim().is_empty())
            })
            .ok_or_else(|| {
                RuntimeError::InvalidRequest(format!(
                    "session `{session_id}` is not bound to a report"
                ))
            })?;
        let doc_id = quire_protocol::DocId::parse(raw_doc_id)
            .map_err(|error| RuntimeError::InvalidRequest(error.to_string()))?;
        match self
            .store
            .report_store()
            .delete_comment_thread(&doc_id, thread_id)
            .await
        {
            Ok(()) | Err(quire_store::StoreError::CommentThreadNotFound { .. }) => {
                self.report_app_state()
                    .publish_comment_deleted(&doc_id, thread_id)
                    .await;
                Ok(())
            }
            Err(error) => Err(RuntimeError::Internal(format!(
                "report comment delete failed: {error}"
            ))),
        }
    }

    async fn emit_queue_update(&self, session_id: &str) -> RuntimeResult<()> {
        self.queue.emit_queue_update(session_id).await
    }

    async fn drain_pending_locked(
        &self,
        session_id: &str,
        force: bool,
    ) -> RuntimeResult<Option<String>> {
        let session = self.store.get_session_summary(session_id).await?;
        if !force
            && matches!(
                session.active_run_status,
                ActiveRunStatus::Running | ActiveRunStatus::AwaitingInput
            )
        {
            return Ok(None);
        }
        if self.store.latest_running_run(session_id).await?.is_some() {
            return Ok(None);
        }
        self.maybe_compact_before_claim(session_id).await;
        let Some((message, run)) = self
            .queue
            .take_pending_batch_for_run_locked(
                session_id,
                &self.instance_id,
                &self.config.default_model,
                15,
                force,
            )
            .await?
        else {
            return Ok(None);
        };
        let token = self.runs.register(&run.run_id).await;
        let model_input = render_stored_user_message(&message);
        self.launch_turn(
            session_id,
            &run.run_id,
            model_input,
            Some(message.message_id),
            token,
        );
        Ok(Some(run.run_id))
    }

    pub(crate) async fn drain_pending(&self, session_id: &str) -> RuntimeResult<Option<String>> {
        let gate = self.session_gate(session_id).await;
        let Ok(_guard) = gate.try_lock_owned() else {
            return Ok(None);
        };
        self.drain_pending_locked(session_id, false).await
    }

    pub(crate) async fn clear_dismissed_questions(
        &self,
        session_id: &str,
        dismissed: Vec<PendingQuestion>,
        reason: &str,
    ) -> RuntimeResult<()> {
        if dismissed.is_empty() {
            return Ok(());
        }
        self.store.clear_session_awaiting_input(session_id).await?;
        for question in dismissed {
            self.emit(
                session_id,
                &question.run_id,
                "main",
                None,
                SessionEventKind::QuestionnaireDismissed {
                    question_id: question.question_id,
                    reason: reason.into(),
                },
            )
            .await?;
        }
        Ok(())
    }

    /// Resolve a pending ask_user card and continue the interrupted reasoning immediately.
    pub async fn answer_question(
        &self,
        session_id: &str,
        question_id: &str,
        answers: &[QuestionAnswer],
        skipped: bool,
    ) -> RuntimeResult<SessionDetail> {
        let gate = self.session_gate(session_id).await;
        let _guard = gate.lock_owned().await;
        let question = self
            .store
            .pending_question_by_id(session_id, question_id)
            .await?
            .ok_or_else(|| {
                RuntimeError::InvalidRequest(format!(
                    "question `{question_id}` is not pending for session `{session_id}`"
                ))
            })?;
        let answers = if skipped {
            Vec::new()
        } else {
            validate_question_answers(&question.questions, answers)?;
            answers.to_vec()
        };
        self.store
            .resolve_question(session_id, question_id, &answers, skipped)
            .await?;
        self.store.clear_session_awaiting_input(session_id).await?;
        self.emit(
            session_id,
            &question.run_id,
            "main",
            None,
            SessionEventKind::QuestionnaireAnswered {
                question_id: question.question_id.clone(),
                answers: answers.clone(),
                skipped_all: skipped,
            },
        )
        .await?;
        let payload = quire_session_protocol::AskUserAnswerPayload::from_resolution(
            &question, &answers, skipped,
        );
        self.spawn_turn(session_id, serde_json::to_string(&payload)?)
            .await?;
        self.store.get_session_detail(session_id).await
    }
}

pub(crate) struct RuntimeExternalHarnessBridge {
    store: RuntimeStore,
}

impl RuntimeExternalHarnessBridge {
    pub(crate) fn new(store: RuntimeStore) -> Self {
        Self { store }
    }
}

fn style_preset_to_template(
    preset: crate::store::StylePresetRecord,
) -> quire_server::state::ExternalReportTemplate {
    let bundle = preset.bundle;
    let mut template = quire_server::state::ExternalReportTemplate {
        theme: bundle.theme,
        ..quire_server::state::ExternalReportTemplate::default()
    };
    if let Some(enabled) = bundle.cover_enabled {
        template.cover.enabled = enabled;
    }
    if let Some(variant) = bundle.cover_variant {
        template.cover.variant = variant;
    }
    if let Some(enabled) = bundle.toc_enabled {
        template.toc.enabled = enabled;
    }
    if let Some(depth) = bundle.toc_max_depth {
        template.toc.max_depth = depth;
    }
    if let Some(variant) = bundle.toc_variant {
        template.toc.variant = variant;
    }
    if let Some(page_break) = bundle.toc_page_break_before {
        template.toc.page_break_before = page_break;
    }
    if let Some(page_break) = bundle.toc_page_break_after {
        template.toc.page_break_after = page_break;
    }
    if let Some(style) = bundle.citation_style {
        template.references.style = style;
    }
    if let Some(enabled) = bundle.references_enabled {
        template.references.enabled = enabled;
    }
    if let Some(page_break) = bundle.references_page_break_before {
        template.references.page_break_before = page_break;
    }
    template
}

impl quire_server::state::ExternalHarnessBridge for RuntimeExternalHarnessBridge {
    fn verify_api_key<'a>(
        &'a self,
        raw_token: &'a str,
    ) -> quire_server::state::BoxFuture<'a, Option<quire_server::state::VerifiedExternalIdentity>>
    {
        Box::pin(async move {
            match self.store.verify_api_key(raw_token).await {
                Ok(Some(user)) => Some(quire_server::state::VerifiedExternalIdentity {
                    user_id: user.user_id,
                    username: user.username,
                    is_admin: user.role == crate::auth::UserRole::Admin,
                    preview_doc_id: None,
                }),
                _ => None,
            }
        })
    }

    fn verify_active_user<'a>(
        &'a self,
        user_id: &'a str,
    ) -> quire_server::state::BoxFuture<'a, Option<quire_server::state::VerifiedExternalIdentity>>
    {
        Box::pin(async move {
            match self.store.verify_active_user(user_id).await {
                Ok(Some(user)) => Some(quire_server::state::VerifiedExternalIdentity {
                    user_id: user.user_id,
                    username: user.username,
                    is_admin: user.role == crate::auth::UserRole::Admin,
                    preview_doc_id: None,
                }),
                _ => None,
            }
        })
    }

    fn resolve_report_template<'a>(
        &'a self,
        theme_preset: Option<&'a str>,
        user_id: Option<&'a str>,
        is_admin: bool,
    ) -> quire_server::state::BoxFuture<
        'a,
        Result<Option<quire_server::state::ExternalReportTemplate>, String>,
    > {
        Box::pin(async move {
            let explicit = theme_preset
                .map(str::trim)
                .filter(|value| !value.is_empty());
            let preset = if let Some(style_id) = explicit {
                self.store.get_style_preset(style_id).await
            } else if let Some(user_id) = user_id {
                self.store
                    .default_style_preset_for_user(user_id)
                    .await
                    .map(Some)
            } else {
                return Ok(None);
            }
            .map_err(|error| error.to_string())?;
            let Some(preset) = preset else {
                return Ok(None);
            };
            if !preset.is_builtin
                && !preset.is_public
                && !is_admin
                && preset.owner_user_id.as_deref() != user_id
            {
                return Err("无权使用该私有样式".to_string());
            }
            Ok(Some(style_preset_to_template(preset)))
        })
    }

    fn prepare_session_for_report<'a>(
        &'a self,
        requested_session_id: Option<&'a quire_protocol::SessionId>,
        user_id: Option<&'a str>,
        is_admin: bool,
        title: &'a str,
    ) -> quire_server::state::BoxFuture<'a, Result<quire_protocol::SessionId, String>> {
        Box::pin(async move {
            let raw_id = self
                .store
                .ensure_external_session_for_report(
                    requested_session_id.map(quire_protocol::SessionId::as_str),
                    user_id,
                    is_admin,
                    title,
                    None,
                )
                .await
                .map_err(|error| error.to_string())?;
            quire_protocol::SessionId::parse(raw_id).map_err(|error| error.to_string())
        })
    }

    fn bind_created_report<'a>(
        &'a self,
        session_id: &'a quire_protocol::SessionId,
        user_id: Option<&'a str>,
        is_admin: bool,
        title: &'a str,
        doc_id: &'a quire_protocol::DocId,
    ) -> quire_server::state::BoxFuture<'a, Result<(), String>> {
        Box::pin(async move {
            let _ = self
                .store
                .ensure_external_session_for_report(
                    Some(session_id.as_str()),
                    user_id,
                    is_admin,
                    title,
                    Some(doc_id.as_str()),
                )
                .await
                .map_err(|error| error.to_string())?;
            Ok(())
        })
    }

    fn authorize_document_access<'a>(
        &'a self,
        doc_id: &'a quire_protocol::DocId,
        doc_session_id: Option<&'a quire_protocol::SessionId>,
        user_id: Option<&'a str>,
        caller_session_id: Option<&'a quire_protocol::SessionId>,
        is_admin: bool,
    ) -> quire_server::state::BoxFuture<'a, Result<(), String>> {
        Box::pin(async move {
            self.store
                .authorize_document_access(
                    doc_id.as_str(),
                    doc_session_id.map(quire_protocol::SessionId::as_str),
                    user_id,
                    caller_session_id.map(quire_protocol::SessionId::as_str),
                    is_admin,
                )
                .await
                .map_err(|error| error.to_string())
        })
    }

    fn authorize_session_access<'a>(
        &'a self,
        session_id: &'a quire_protocol::SessionId,
        user_id: Option<&'a str>,
        is_admin: bool,
    ) -> quire_server::state::BoxFuture<'a, Result<(), String>> {
        Box::pin(async move {
            self.store
                .authorize_session_access(session_id.as_str(), user_id, is_admin)
                .await
                .map_err(|error| error.to_string())
        })
    }

    fn accessible_report_scope<'a>(
        &'a self,
        user_id: Option<&'a str>,
        caller_session_id: Option<&'a quire_protocol::SessionId>,
        is_admin: bool,
    ) -> quire_server::state::BoxFuture<'a, Result<Option<(Vec<String>, Vec<String>)>, String>>
    {
        Box::pin(async move {
            self.store
                .accessible_report_scope(
                    user_id,
                    caller_session_id.map(quire_protocol::SessionId::as_str),
                    is_admin,
                )
                .await
                .map_err(|error| error.to_string())
        })
    }
}
