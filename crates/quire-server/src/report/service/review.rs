use super::*;

impl QuireReportService {
    pub async fn exec_add_comment(
        &self,
        context: RequestContext,
        request: AddCommentParams,
    ) -> Result<AddCommentReceipt, ErrorData> {
        let actor = context.actor;
        let doc_id = parse_doc_id(&request.doc_id)?;
        let block_id = parse_block_id(&request.block_id)?;
        let end_block_id = request
            .end_block_id
            .as_deref()
            .map(parse_block_id)
            .transpose()?;
        let anchor_blocks = request
            .anchor_blocks
            .iter()
            .map(|raw| parse_block_id(raw))
            .collect::<Result<Vec<_>, _>>()?;
        if let Some(quote) = &request.quote {
            quote.validate().map_err(|reason| {
                invalid_params(
                    reason,
                    "Quote the exact selected phrase; prefix/suffix provide context.",
                    None,
                )
            })?;
        }
        if let Some(end_quote) = &request.end_quote {
            end_quote.validate().map_err(|reason| {
                invalid_params(
                    reason,
                    "Quote the exact phrase in the end block; prefix/suffix provide context.",
                    None,
                )
            })?;
        }
        let thread = self
            .state
            .store
            .create_comment_thread(
                &doc_id,
                NewCommentThread {
                    block_id,
                    end_block_id,
                    anchor_blocks,
                    author_type: CommentAuthorType::Agent,
                    author: actor,
                    body: request.body,
                    quote: request.quote,
                    end_quote: request.end_quote,
                    anchor_snapshot: request.anchor_snapshot,
                },
            )
            .await
            .map_err(mcp_error)?;
        self.state.publish_comment_updated(&thread).await;
        let _next_hint = format!(
            "Comment thread `{}` created without a revision change. Writers should reply with reply_comment after editing.",
            thread.thread_id
        );
        Ok(AddCommentReceipt { thread, _next_hint })
    }

    pub async fn exec_reply_comment(
        &self,
        context: RequestContext,
        request: ReplyCommentParams,
    ) -> Result<ReplyCommentReceipt, ErrorData> {
        let actor = context.actor;
        let doc_id = parse_doc_id(&request.doc_id)?;
        let addressed_rev = request.addressed_rev.map(parse_revision).transpose()?;
        let thread = self
            .state
            .store
            .reply_comment_thread(
                &doc_id,
                &request.thread_id,
                NewCommentReply {
                    author_type: CommentAuthorType::Agent,
                    author: actor,
                    body: request.body,
                    mark_addressed: request.mark_addressed,
                    addressed_rev,
                },
            )
            .await
            .map_err(mcp_error)?;
        self.state.publish_comment_updated(&thread).await;
        let _next_hint = match thread.status {
            CommentStatus::Addressed => format!(
                "Thread marked addressed at revision {}. Only a human can archive it through PATCH /api/docs/{doc_id}/comments/{{thread_id}}.",
                thread
                    .addressed_rev
                    .map(|revision| revision.get())
                    .unwrap_or(0)
            ),
            CommentStatus::Archived => {
                "Reply recorded on an archived thread; the thread stays archived.".to_string()
            }
            CommentStatus::Open => {
                "Reply recorded. Set mark_addressed=true after the fix to advance the thread."
                    .to_string()
            }
        };
        Ok(ReplyCommentReceipt { thread, _next_hint })
    }
}
