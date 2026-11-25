use crate::auth::{AuthUser, UserRole};
use axum::Extension;

use super::*;

async fn ensure_access(
    service: &RuntimeService,
    session_id: &str,
    auth_user: &AuthUser,
) -> RuntimeResult<()> {
    service
        .ensure_session_access(
            session_id,
            &auth_user.user_id,
            auth_user.role == UserRole::Admin,
        )
        .await
}

#[derive(Debug, Deserialize)]
pub(crate) struct PageQuery {
    limit: Option<i64>,
    offset: Option<i64>,
}

pub(crate) async fn list_sessions(
    State(service): State<RuntimeService>,
    Extension(auth_user): Extension<AuthUser>,
    Query(query): Query<PageQuery>,
) -> RuntimeResult<Json<Vec<SessionSummary>>> {
    service
        .list_sessions_for_user(
            Some(&auth_user.user_id),
            auth_user.role == UserRole::Admin,
            query.limit.unwrap_or(50),
            query.offset.unwrap_or(0),
        )
        .await
        .map(Json)
}

pub(crate) async fn create_session(
    State(service): State<RuntimeService>,
    Extension(auth_user): Extension<AuthUser>,
    Json(request): Json<CreateSessionRequest>,
) -> RuntimeResult<Json<SessionSummary>> {
    service
        .create_session_for_user(
            Some(&auth_user.user_id),
            &request.title,
            request.doc_id.as_deref(),
            request.kb_branch.as_deref(),
        )
        .await
        .map(Json)
}

pub(crate) async fn get_session(
    State(service): State<RuntimeService>,
    Extension(auth_user): Extension<AuthUser>,
    Path(session_id): Path<String>,
) -> RuntimeResult<Json<SessionDetail>> {
    ensure_access(&service, &session_id, &auth_user).await?;
    service.session_detail(&session_id).await.map(Json)
}

pub(crate) async fn get_chart_preview(
    State(service): State<RuntimeService>,
    Path(preview_id): Path<String>,
) -> RuntimeResult<Json<ChartPreviewRecord>> {
    if let Some(chart) = service.store().load_chat_chart(&preview_id).await? {
        return Ok(Json(chart));
    }
    service
        .store()
        .bus()
        .load_chart_preview(&preview_id)
        .await?
        .map(Json)
        .ok_or_else(|| RuntimeError::ChartPreviewNotFound(preview_id))
}

pub(crate) async fn list_session_traces(
    State(service): State<RuntimeService>,
    Extension(auth_user): Extension<AuthUser>,
    Path(session_id): Path<String>,
) -> RuntimeResult<Json<Vec<RunTraceSummary>>> {
    ensure_access(&service, &session_id, &auth_user).await?;
    service.list_run_traces(&session_id).await.map(Json)
}

#[derive(Debug, Deserialize)]
pub(crate) struct UpdateSessionRequest {
    #[serde(default)]
    title: Option<String>,
    #[serde(default)]
    status: Option<quire_session_protocol::SessionStatus>,
    #[serde(default)]
    pinned: Option<bool>,
}

pub(crate) async fn update_session(
    State(service): State<RuntimeService>,
    Extension(auth_user): Extension<AuthUser>,
    Path(session_id): Path<String>,
    Json(request): Json<UpdateSessionRequest>,
) -> RuntimeResult<Json<SessionSummary>> {
    ensure_access(&service, &session_id, &auth_user).await?;
    service
        .update_session(
            &session_id,
            SessionPatch {
                title: request.title.as_deref(),
                status: request.status,
                pinned: request.pinned,
            },
        )
        .await
        .map(Json)
}

pub(crate) async fn delete_session(
    State(service): State<RuntimeService>,
    Extension(auth_user): Extension<AuthUser>,
    Path(session_id): Path<String>,
) -> RuntimeResult<StatusCode> {
    ensure_access(&service, &session_id, &auth_user).await?;
    service.delete_session(&session_id).await?;
    Ok(StatusCode::NO_CONTENT)
}

#[derive(Debug, Serialize)]
pub(crate) struct PostMessageResponse {
    #[serde(skip_serializing_if = "Option::is_none")]
    message: Option<quire_session_protocol::SessionMessage>,
    inserted: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    run_id: Option<String>,
    queued: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    queue_item: Option<PendingQueueItem>,
}

pub(crate) async fn post_message(
    State(service): State<RuntimeService>,
    Extension(auth_user): Extension<AuthUser>,
    Path(session_id): Path<String>,
    Json(request): Json<SendMessageRequest>,
) -> RuntimeResult<Json<PostMessageResponse>> {
    ensure_access(&service, &session_id, &auth_user).await?;
    let outcome = service.post_message(&session_id, &request).await?;
    let response = match outcome {
        PostMessageOutcome::Started { message, run_id } => PostMessageResponse {
            message: Some(message),
            inserted: true,
            run_id: Some(run_id),
            queued: false,
            queue_item: None,
        },
        PostMessageOutcome::Replayed { message } => PostMessageResponse {
            message: Some(message),
            inserted: false,
            run_id: None,
            queued: false,
            queue_item: None,
        },
        PostMessageOutcome::Queued { item } => PostMessageResponse {
            message: None,
            inserted: true,
            run_id: None,
            queued: true,
            queue_item: Some(item),
        },
    };
    Ok(Json(response))
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct SessionCommentCreateRequest {
    block_id: String,
    #[serde(default)]
    end_block_id: Option<String>,
    #[serde(default)]
    anchor_blocks: Vec<String>,
    body: String,
    #[serde(default)]
    quote: Option<TextQuoteSelector>,
    #[serde(default)]
    end_quote: Option<TextQuoteSelector>,
    #[serde(default)]
    anchor_snapshot: Option<String>,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct SessionCommentReplyRequest {
    body: String,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct SessionCommentStatusRequest {
    status: String,
}

// Queue updates use the shared session wire contract.

#[derive(Debug)]
pub(crate) struct CommentProxyError {
    status: StatusCode,
    code: &'static str,
    message: String,
    hint: &'static str,
}

impl IntoResponse for CommentProxyError {
    fn into_response(self) -> Response {
        (
            self.status,
            Json(ApiErrorEnvelope::<()>::new(
                self.code,
                self.message,
                self.hint,
                None,
            )),
        )
            .into_response()
    }
}

fn proxy_runtime_error(error: RuntimeError) -> CommentProxyError {
    let status = match error {
        RuntimeError::SessionNotFound(_) => StatusCode::NOT_FOUND,
        RuntimeError::InvalidRequest(_) => StatusCode::BAD_REQUEST,
        _ => StatusCode::INTERNAL_SERVER_ERROR,
    };
    CommentProxyError {
        status,
        code: error.code(),
        message: error.to_string(),
        hint: error.hint(),
    }
}

fn store_comment_error(error: quire_store::StoreError) -> CommentProxyError {
    let status = match &error {
        quire_store::StoreError::NotFound { .. }
        | quire_store::StoreError::CommentThreadNotFound { .. }
        | quire_store::StoreError::CommentTargetNotFound { .. } => StatusCode::NOT_FOUND,
        quire_store::StoreError::InvalidDocId { .. }
        | quire_store::StoreError::InvalidRequest(_)
        | quire_store::StoreError::Protocol(_) => StatusCode::BAD_REQUEST,
        _ => StatusCode::INTERNAL_SERVER_ERROR,
    };
    CommentProxyError {
        status,
        code: error.code(),
        message: error.to_string(),
        hint: "Inspect the comment payload and the bound report, then retry.",
    }
}

pub(crate) async fn create_session_comment(
    State(service): State<RuntimeService>,
    Extension(auth_user): Extension<AuthUser>,
    Path(session_id): Path<String>,
    Json(request): Json<SessionCommentCreateRequest>,
) -> Result<Json<CommentThread>, CommentProxyError> {
    ensure_access(&service, &session_id, &auth_user)
        .await
        .map_err(proxy_runtime_error)?;
    let raw_doc_id = session_comment_doc_id(&service, &session_id)
        .await
        .map_err(proxy_runtime_error)?;
    let doc_id = quire_protocol::DocId::parse(raw_doc_id)
        .map_err(|error| store_comment_error(quire_store::StoreError::Protocol(error)))?;
    let block_id = quire_protocol::BlockId::parse(request.block_id)
        .map_err(|error| store_comment_error(quire_store::StoreError::Protocol(error)))?;
    let end_block_id = request
        .end_block_id
        .map(quire_protocol::BlockId::parse)
        .transpose()
        .map_err(|error| store_comment_error(quire_store::StoreError::Protocol(error)))?;
    let anchor_blocks = request
        .anchor_blocks
        .into_iter()
        .map(quire_protocol::BlockId::parse)
        .collect::<Result<Vec<_>, _>>()
        .map_err(|error| store_comment_error(quire_store::StoreError::Protocol(error)))?;
    if let Some(quote) = &request.quote {
        quote.validate().map_err(|reason| {
            store_comment_error(quire_store::StoreError::InvalidRequest(reason))
        })?;
    }
    if let Some(end_quote) = &request.end_quote {
        end_quote.validate().map_err(|reason| {
            store_comment_error(quire_store::StoreError::InvalidRequest(reason))
        })?;
    }
    let thread = service
        .store()
        .report_store()
        .create_comment_thread(
            &doc_id,
            quire_store::NewCommentThread {
                block_id,
                end_block_id,
                anchor_blocks,
                author_type: quire_protocol::CommentAuthorType::Human,
                author: "reviewer".into(),
                body: request.body,
                quote: request.quote,
                end_quote: request.end_quote,
                anchor_snapshot: request.anchor_snapshot,
                evaluation_meta: None,
            },
        )
        .await
        .map_err(store_comment_error)?;
    service
        .report_app_state()
        .publish_comment_updated(&thread)
        .await;
    let comment_ref = session_comment_ref(&thread);
    if let Err(error) = service.post_comment_message(&session_id, comment_ref).await {
        tracing::warn!(%error, %session_id, thread_id = %thread.thread_id, "failed to auto-trigger comment review");
    }
    Ok(Json(thread))
}

pub(crate) async fn reply_session_comment(
    State(service): State<RuntimeService>,
    Extension(auth_user): Extension<AuthUser>,
    Path((session_id, thread_id)): Path<(String, String)>,
    Json(request): Json<SessionCommentReplyRequest>,
) -> Result<Json<CommentThread>, CommentProxyError> {
    ensure_access(&service, &session_id, &auth_user)
        .await
        .map_err(proxy_runtime_error)?;
    let raw_doc_id = session_comment_doc_id(&service, &session_id)
        .await
        .map_err(proxy_runtime_error)?;
    let doc_id = quire_protocol::DocId::parse(raw_doc_id)
        .map_err(|error| store_comment_error(quire_store::StoreError::Protocol(error)))?;
    let thread = service
        .store()
        .report_store()
        .reply_comment_thread(
            &doc_id,
            &thread_id,
            quire_store::NewCommentReply {
                author_type: quire_protocol::CommentAuthorType::Human,
                author: "reviewer".into(),
                body: request.body,
                mark_addressed: false,
                addressed_rev: None,
            },
        )
        .await
        .map_err(store_comment_error)?;
    service
        .report_app_state()
        .publish_comment_updated(&thread)
        .await;
    if thread.status != CommentStatus::Archived
        && let Some(reply) = thread.messages.last()
    {
        let comment_ref = session_comment_ref_with_body(&thread, &reply.body);
        if let Err(error) = service
            .post_comment_reply_message(&session_id, &reply.message_id, comment_ref)
            .await
        {
            tracing::warn!(
                %error,
                %session_id,
                thread_id = %thread.thread_id,
                "failed to auto-trigger agent for a comment follow-up"
            );
        }
    }
    Ok(Json(thread))
}

pub(crate) async fn update_session_comment_status(
    State(service): State<RuntimeService>,
    Extension(auth_user): Extension<AuthUser>,
    Path((session_id, thread_id)): Path<(String, String)>,
    Json(request): Json<SessionCommentStatusRequest>,
) -> Result<Json<CommentThread>, CommentProxyError> {
    ensure_access(&service, &session_id, &auth_user)
        .await
        .map_err(proxy_runtime_error)?;
    let raw_doc_id = session_comment_doc_id(&service, &session_id)
        .await
        .map_err(proxy_runtime_error)?;
    let doc_id = quire_protocol::DocId::parse(raw_doc_id)
        .map_err(|error| store_comment_error(quire_store::StoreError::Protocol(error)))?;
    let status = CommentStatus::parse(&request.status).ok_or_else(|| {
        store_comment_error(quire_store::StoreError::InvalidRequest(format!(
            "unknown comment status `{}`; expected open, addressed or archived",
            request.status
        )))
    })?;
    let thread = service
        .store()
        .report_store()
        .update_comment_status(&doc_id, &thread_id, status)
        .await
        .map_err(store_comment_error)?;
    service
        .report_app_state()
        .publish_comment_updated(&thread)
        .await;
    Ok(Json(thread))
}

pub(crate) async fn delete_session_comment(
    State(service): State<RuntimeService>,
    Extension(auth_user): Extension<AuthUser>,
    Path((session_id, thread_id)): Path<(String, String)>,
) -> RuntimeResult<StatusCode> {
    ensure_access(&service, &session_id, &auth_user).await?;
    let raw_doc_id = session_comment_doc_id(&service, &session_id).await?;
    let doc_id = quire_protocol::DocId::parse(raw_doc_id)
        .map_err(|error| RuntimeError::InvalidRequest(error.to_string()))?;
    service
        .remove_pending_comment_threads(&session_id, &thread_id)
        .await?;
    match service
        .store()
        .report_store()
        .delete_comment_thread(&doc_id, &thread_id)
        .await
    {
        Ok(()) | Err(quire_store::StoreError::CommentThreadNotFound { .. }) => {
            service
                .report_app_state()
                .publish_comment_deleted(&doc_id, &thread_id)
                .await;
            Ok(StatusCode::NO_CONTENT)
        }
        Err(error) => Err(RuntimeError::Internal(error.to_string())),
    }
}

pub(crate) async fn list_session_queue(
    State(service): State<RuntimeService>,
    Extension(auth_user): Extension<AuthUser>,
    Path(session_id): Path<String>,
) -> RuntimeResult<Json<Vec<PendingQueueItem>>> {
    ensure_access(&service, &session_id, &auth_user).await?;
    service.list_pending_queue(&session_id).await.map(Json)
}

pub(crate) async fn update_session_queue_item(
    State(service): State<RuntimeService>,
    Extension(auth_user): Extension<AuthUser>,
    Path((session_id, queue_id)): Path<(String, String)>,
    Json(request): Json<UpdatePendingMessageRequest>,
) -> RuntimeResult<Json<PendingQueueItem>> {
    ensure_access(&service, &session_id, &auth_user).await?;
    service
        .update_pending_message(
            &session_id,
            &queue_id,
            &request.content,
            &request.comment_refs,
        )
        .await
        .map(Json)
}

pub(crate) async fn delete_session_queue_item(
    State(service): State<RuntimeService>,
    Extension(auth_user): Extension<AuthUser>,
    Path((session_id, queue_id)): Path<(String, String)>,
) -> RuntimeResult<StatusCode> {
    ensure_access(&service, &session_id, &auth_user).await?;
    if service
        .delete_pending_message(&session_id, &queue_id)
        .await?
    {
        Ok(StatusCode::NO_CONTENT)
    } else {
        Err(RuntimeError::PendingQueueItemNotFound(queue_id))
    }
}

pub(crate) async fn session_comment_doc_id(
    service: &RuntimeService,
    session_id: &str,
) -> RuntimeResult<String> {
    let session = service.store().get_session_summary(session_id).await?;
    session
        .doc_id
        .filter(|doc_id| !doc_id.trim().is_empty())
        .or_else(|| {
            session
                .linked_doc_id
                .filter(|doc_id| !doc_id.trim().is_empty())
        })
        .ok_or_else(|| {
            RuntimeError::InvalidRequest(format!(
                "session `{session_id}` is not bound to a report; create or link a report before commenting"
            ))
        })
}

fn session_comment_ref(thread: &CommentThread) -> SessionCommentRef {
    let comment = thread
        .messages
        .first()
        .map(|message| message.body.as_str())
        .unwrap_or_default();
    session_comment_ref_with_body(thread, comment)
}

fn session_comment_ref_with_body(thread: &CommentThread, comment: &str) -> SessionCommentRef {
    SessionCommentRef {
        thread_id: thread.thread_id.clone(),
        block_id: thread.block_id.as_str().to_string(),
        end_block_id: thread
            .end_block_id
            .as_ref()
            .map(|block_id| block_id.as_str().to_string()),
        anchor_blocks: thread
            .anchor_blocks
            .iter()
            .map(|block_id| block_id.as_str().to_string())
            .collect(),
        status: thread.status.as_str().to_string(),
        quote: thread.quote.as_ref().map(|quote| quote.exact.clone()),
        end_quote: thread.end_quote.as_ref().map(|quote| quote.exact.clone()),
        anchor_snapshot: thread.anchor_snapshot.clone(),
        comment: comment.to_string(),
    }
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct AnswerQuestionRequest {
    #[serde(default)]
    answers: Vec<QuestionAnswer>,
    #[serde(default)]
    skipped: bool,
}

pub(crate) async fn answer_question(
    State(service): State<RuntimeService>,
    Extension(auth_user): Extension<AuthUser>,
    Path((session_id, question_id)): Path<(String, String)>,
    Json(request): Json<AnswerQuestionRequest>,
) -> RuntimeResult<Json<SessionDetail>> {
    ensure_access(&service, &session_id, &auth_user).await?;
    service
        .answer_question(&session_id, &question_id, &request.answers, request.skipped)
        .await
        .map(Json)
}

#[derive(Debug, Default, Deserialize)]
pub(crate) struct InterruptRequest {
    #[serde(default)]
    run_id: Option<String>,
}

pub(crate) async fn interrupt(
    State(service): State<RuntimeService>,
    Extension(auth_user): Extension<AuthUser>,
    Path(session_id): Path<String>,
    request: Option<Json<InterruptRequest>>,
) -> RuntimeResult<Json<SessionDetail>> {
    ensure_access(&service, &session_id, &auth_user).await?;
    let run_id = request.as_ref().and_then(|json| json.0.run_id.as_deref());
    service.interrupt(&session_id, run_id).await.map(Json)
}

pub(crate) async fn retry(
    State(service): State<RuntimeService>,
    Extension(auth_user): Extension<AuthUser>,
    Path(session_id): Path<String>,
) -> RuntimeResult<Json<SessionDetail>> {
    ensure_access(&service, &session_id, &auth_user).await?;
    service.retry(&session_id).await.map(Json)
}

pub(crate) async fn compact(
    State(service): State<RuntimeService>,
    Extension(auth_user): Extension<AuthUser>,
    Path(session_id): Path<String>,
) -> RuntimeResult<Json<SessionDetail>> {
    ensure_access(&service, &session_id, &auth_user).await?;
    let task = tokio::spawn(async move { service.compact(&session_id).await });
    task.await
        .map_err(|error| RuntimeError::Internal(format!("compaction task failed: {error}")))?
        .map(Json)
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct AttachmentPresignRequest {
    filename: String,
    mime_type: String,
    size_bytes: u64,
}

#[derive(Debug, Deserialize)]
pub(crate) struct AttachmentUpload {
    #[serde(default)]
    session_id: Option<String>,
    filename: String,
    mime_type: String,
    size_bytes: u64,
    #[serde(default)]
    content_base64: Option<String>,
    #[serde(default)]
    attachment_id: Option<String>,
    #[serde(default)]
    object_key: Option<String>,
}

pub(crate) async fn presign_session_attachment(
    State(service): State<RuntimeService>,
    Extension(auth_user): Extension<AuthUser>,
    Path(session_id): Path<String>,
    Json(request): Json<AttachmentPresignRequest>,
) -> RuntimeResult<Json<AttachmentPresign>> {
    ensure_access(&service, &session_id, &auth_user).await?;
    service
        .presign_attachment(
            &session_id,
            &request.filename,
            &request.mime_type,
            request.size_bytes,
        )
        .await
        .map(Json)
}

pub(crate) async fn post_attachment(
    State(service): State<RuntimeService>,
    Extension(auth_user): Extension<AuthUser>,
    Json(request): Json<AttachmentUpload>,
) -> RuntimeResult<Json<AttachmentMeta>> {
    let session_id = request.session_id.clone().ok_or_else(|| {
        RuntimeError::InvalidRequest("session_id is required for /attachments".into())
    })?;
    ensure_access(&service, &session_id, &auth_user).await?;
    attach(&service, &session_id, request).await
}

pub(crate) async fn post_session_attachment(
    State(service): State<RuntimeService>,
    Extension(auth_user): Extension<AuthUser>,
    Path(session_id): Path<String>,
    Json(request): Json<AttachmentUpload>,
) -> RuntimeResult<Json<AttachmentMeta>> {
    ensure_access(&service, &session_id, &auth_user).await?;
    attach(&service, &session_id, request).await
}

pub(crate) async fn attach(
    service: &RuntimeService,
    session_id: &str,
    request: AttachmentUpload,
) -> RuntimeResult<Json<AttachmentMeta>> {
    service
        .create_attachment(AttachmentWrite {
            session_id,
            attachment_id: request.attachment_id.as_deref(),
            filename: &request.filename,
            mime_type: &request.mime_type,
            size_bytes: request.size_bytes,
            content_base64: request.content_base64.as_deref(),
            object_key: request.object_key.as_deref(),
        })
        .await
        .map(Json)
}

pub(crate) async fn get_tool_call(
    State(service): State<RuntimeService>,
    Extension(auth_user): Extension<AuthUser>,
    Path((session_id, tool_call_id)): Path<(String, String)>,
) -> RuntimeResult<Json<ToolCallDetail>> {
    ensure_access(&service, &session_id, &auth_user).await?;
    service
        .get_tool_call(&session_id, &tool_call_id)
        .await
        .map(Json)
}
