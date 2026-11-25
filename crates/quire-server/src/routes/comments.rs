use axum::{
    Json,
    extract::{Path, Query, State},
    http::StatusCode,
};
use quire_protocol::DocId;
use quire_protocol::{BlockId, CommentAuthorType, CommentStatus, CommentThread, TextQuoteSelector};
use quire_store::{NewCommentReply, NewCommentThread, StoreError};
use serde::{Deserialize, Serialize};

use crate::{auth::RequestContext, error::ApiError, state::AppState};

#[derive(Debug, Serialize)]
pub(super) struct CommentListView {
    doc_id: DocId,
    comments: Vec<CommentThread>,
}

#[derive(Debug, Deserialize)]
pub(super) struct CommentListQuery {
    status: Option<String>,
    block_id: Option<String>,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub(super) struct CreateCommentRequest {
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
pub(super) struct ReplyCommentRequest {
    body: String,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub(super) struct UpdateCommentStatusRequest {
    status: String,
}

fn parse_comment_status(value: &str) -> Result<CommentStatus, ApiError> {
    CommentStatus::parse(value).ok_or_else(|| {
        ApiError(StoreError::InvalidRequest(format!(
            "unknown comment status `{value}`; expected open, addressed, archived or all"
        )))
    })
}

pub(super) async fn list_comments(
    State(state): State<AppState>,
    Path(doc_id): Path<DocId>,
    context: RequestContext,
    Query(query): Query<CommentListQuery>,
) -> Result<Json<CommentListView>, ApiError> {
    state.authorize_doc_access(&context, &doc_id, false).await?;
    let status = match query.status.as_deref() {
        None | Some("all") => None,
        Some(raw) => Some(parse_comment_status(raw)?),
    };
    let block_id = query.block_id.as_deref().map(BlockId::parse).transpose()?;
    let mut comments = state
        .store
        .list_comment_threads(&doc_id, status, block_id.as_ref())
        .await?;
    if query.status.is_none() {
        comments.retain(|thread| thread.status != CommentStatus::Archived);
    }
    Ok(Json(CommentListView { doc_id, comments }))
}

pub(super) async fn create_comment(
    State(state): State<AppState>,
    Path(doc_id): Path<DocId>,
    context: RequestContext,
    Json(request): Json<CreateCommentRequest>,
) -> Result<Json<CommentThread>, ApiError> {
    state.authorize_doc_access(&context, &doc_id, true).await?;
    let block_id = BlockId::parse(request.block_id)?;
    let end_block_id = request.end_block_id.map(BlockId::parse).transpose()?;
    let anchor_blocks = request
        .anchor_blocks
        .into_iter()
        .map(BlockId::parse)
        .collect::<Result<Vec<_>, _>>()?;
    if let Some(quote) = &request.quote {
        quote
            .validate()
            .map_err(|reason| ApiError(StoreError::InvalidRequest(reason)))?;
    }
    if let Some(end_quote) = &request.end_quote {
        end_quote
            .validate()
            .map_err(|reason| ApiError(StoreError::InvalidRequest(reason)))?;
    }
    let thread = state
        .store
        .create_comment_thread(
            &doc_id,
            NewCommentThread {
                block_id,
                end_block_id,
                anchor_blocks,
                author_type: CommentAuthorType::Human,
                author: context.actor,
                body: request.body,
                quote: request.quote,
                end_quote: request.end_quote,
                anchor_snapshot: request.anchor_snapshot,
                evaluation_meta: None,
            },
        )
        .await?;
    state.publish_comment_updated(&thread).await;
    Ok(Json(thread))
}

pub(super) async fn reply_comment(
    State(state): State<AppState>,
    Path((doc_id, thread_id)): Path<(DocId, String)>,
    context: RequestContext,
    Json(request): Json<ReplyCommentRequest>,
) -> Result<Json<CommentThread>, ApiError> {
    state.authorize_doc_access(&context, &doc_id, true).await?;
    let thread = state
        .store
        .reply_comment_thread(
            &doc_id,
            &thread_id,
            NewCommentReply {
                author_type: CommentAuthorType::Human,
                author: context.actor,
                body: request.body,
                mark_addressed: false,
                addressed_rev: None,
            },
        )
        .await?;
    state.publish_comment_updated(&thread).await;
    Ok(Json(thread))
}

pub(super) async fn update_comment_status(
    State(state): State<AppState>,
    Path((doc_id, thread_id)): Path<(DocId, String)>,
    context: RequestContext,
    Json(request): Json<UpdateCommentStatusRequest>,
) -> Result<Json<CommentThread>, ApiError> {
    state.authorize_doc_access(&context, &doc_id, true).await?;
    let status = parse_comment_status(&request.status)?;
    let thread = state
        .store
        .update_comment_status(&doc_id, &thread_id, status)
        .await?;
    state.publish_comment_updated(&thread).await;
    Ok(Json(thread))
}

pub(super) async fn delete_comment(
    State(state): State<AppState>,
    Path((doc_id, thread_id)): Path<(DocId, String)>,
    context: RequestContext,
) -> Result<StatusCode, ApiError> {
    state.authorize_doc_access(&context, &doc_id, true).await?;
    state
        .store
        .delete_comment_thread(&doc_id, &thread_id)
        .await?;
    state.publish_comment_deleted(&doc_id, &thread_id).await;
    Ok(StatusCode::NO_CONTENT)
}
