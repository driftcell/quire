use super::*;

impl PgReportStore {
    pub async fn create_comment_thread(
        &self,
        doc_id: &DocId,
        request: NewCommentThread,
    ) -> Result<CommentThread, StoreError> {
        validate_comment_body(&request.body)?;
        if let Some(quote) = &request.quote {
            quote.validate().map_err(StoreError::InvalidRequest)?;
        }
        if let Some(end_quote) = &request.end_quote {
            end_quote.validate().map_err(StoreError::InvalidRequest)?;
        }
        let mut tx = self.pool.begin().await?;
        let loaded = load_document_in_tx(&mut tx, doc_id.as_str()).await?;
        let document = loaded.document;
        let index = document
            .blocks
            .iter()
            .position(|block| block.id == request.block_id)
            .ok_or_else(|| StoreError::CommentTargetNotFound {
                doc_id: doc_id.clone(),
                block_id: request.block_id.clone(),
            })?;
        if let Some(end_block_id) = &request.end_block_id
            && !document
                .blocks
                .iter()
                .any(|block| &block.id == end_block_id)
        {
            return Err(StoreError::CommentTargetNotFound {
                doc_id: doc_id.clone(),
                block_id: end_block_id.clone(),
            });
        }
        let block =
            document
                .blocks
                .get(index)
                .ok_or_else(|| StoreError::CommentTargetNotFound {
                    doc_id: doc_id.clone(),
                    block_id: request.block_id.clone(),
                })?;
        let section_id = comment_section_for_index(&document, index);
        let anchor_snapshot = request
            .anchor_snapshot
            .as_deref()
            .map(str::trim)
            .filter(|snapshot| !snapshot.is_empty())
            .map(str::to_string)
            .or_else(|| comment_anchor_snapshot(block));
        let anchor_blocks = normalize_anchor_blocks(
            &document,
            &request.block_id,
            request.end_block_id.as_ref(),
            &request.anchor_blocks,
        );
        let quote = request
            .quote
            .as_ref()
            .map(serde_json::to_value)
            .transpose()?;
        let end_quote = request
            .end_quote
            .as_ref()
            .map(serde_json::to_value)
            .transpose()?;
        let anchor_blocks_json = serde_json::to_value(&anchor_blocks)?;
        let now = Utc::now();
        let thread_id = short_id("th");
        let message_id = short_id("msg");
        let status = CommentStatus::Open;

        sqlx::query!(
            r#"INSERT INTO report_comment_threads
               (doc_id, thread_id, block_id, end_block_id, anchor_blocks, section_id,
                anchor_snapshot, quote, end_quote, status, created_rev, addressed_rev,
                created_at, updated_at)
               VALUES ($1, $2, $3, $4, $5, $6, $7, $8, $9, $10, $11, NULL, $12, $12)"#,
            doc_id.as_str(),
            &thread_id,
            request.block_id.as_str(),
            request.end_block_id.as_ref().map(BlockId::as_str),
            anchor_blocks_json,
            section_id.as_ref().map(BlockId::as_str),
            anchor_snapshot.as_deref(),
            quote,
            end_quote,
            status.as_str(),
            document.revision.get(),
            now,
        )
        .execute(&mut *tx)
        .await?;
        sqlx::query!(
            r#"INSERT INTO report_comment_messages
               (message_id, doc_id, thread_id, author_type, author, body, created_at)
               VALUES ($1, $2, $3, $4, $5, $6, $7)"#,
            &message_id,
            doc_id.as_str(),
            &thread_id,
            request.author_type.as_str(),
            &request.author,
            &request.body,
            now,
        )
        .execute(&mut *tx)
        .await?;
        tx.commit().await?;

        Ok(CommentThread {
            thread_id: thread_id.clone(),
            doc_id: doc_id.clone(),
            block_id: request.block_id,
            end_block_id: request.end_block_id,
            anchor_blocks,
            section_id,
            anchor_snapshot,
            quote: request.quote,
            end_quote: request.end_quote,
            status,
            created_rev: document.revision,
            addressed_rev: None,
            anchor_detached: false,
            messages: vec![CommentMessage {
                message_id,
                thread_id,
                author_type: request.author_type,
                author: request.author,
                body: request.body,
                created_at: now,
            }],
            created_at: now,
            updated_at: now,
        })
    }

    /// List review threads with live detached-anchor detection.
    pub async fn list_comment_threads(
        &self,
        doc_id: &DocId,
        status: Option<CommentStatus>,
        block_id: Option<&BlockId>,
    ) -> Result<Vec<CommentThread>, StoreError> {
        let rows = sqlx::query!(
            r#"SELECT t.doc_id, t.thread_id, t.block_id, t.end_block_id, t.anchor_blocks,
                      t.section_id, t.anchor_snapshot, t.quote, t.end_quote, t.status,
                      t.created_rev, t.addressed_rev, t.created_at, t.updated_at,
                      (NOT EXISTS (
                          SELECT 1 FROM report_blocks b
                          WHERE b.doc_id = t.doc_id
                            AND (b.block_id = t.block_id
                                 OR b.block_id = t.end_block_id
                                 OR b.block_id IN (
                                     SELECT jsonb_array_elements_text(t.anchor_blocks)
                                 ))
                      )) AS "anchor_detached!"
               FROM report_comment_threads t
               WHERE t.doc_id = $1
                 AND ($2::text IS NULL OR t.status = $2)
                 AND ($3::text IS NULL
                      OR t.block_id = $3
                      OR t.end_block_id = $3
                      OR t.anchor_blocks ? $3)
               ORDER BY t.updated_at DESC, t.thread_id ASC"#,
            doc_id.as_str(),
            status.map(CommentStatus::as_str),
            block_id.map(BlockId::as_str),
        )
        .fetch_all(&self.pool)
        .await?;
        let message_rows = sqlx::query!(
            r#"SELECT message_id, thread_id, author_type, author, body, created_at
               FROM report_comment_messages
               WHERE doc_id = $1
               ORDER BY created_at ASC, message_id ASC"#,
            doc_id.as_str()
        )
        .fetch_all(&self.pool)
        .await?;
        let mut messages_by_thread: HashMap<String, Vec<CommentMessage>> = HashMap::new();
        for row in message_rows {
            let message = build_comment_message(
                row.message_id,
                row.thread_id.clone(),
                &row.author_type,
                row.author,
                row.body,
                row.created_at,
            )?;
            messages_by_thread
                .entry(row.thread_id)
                .or_default()
                .push(message);
        }
        rows.into_iter()
            .map(|row| {
                let row = CommentThreadRow {
                    doc_id: row.doc_id,
                    thread_id: row.thread_id,
                    block_id: row.block_id,
                    end_block_id: row.end_block_id,
                    anchor_blocks: row.anchor_blocks,
                    section_id: row.section_id,
                    anchor_snapshot: row.anchor_snapshot,
                    quote: row.quote,
                    end_quote: row.end_quote,
                    status: row.status,
                    created_rev: row.created_rev,
                    addressed_rev: row.addressed_rev,
                    created_at: row.created_at,
                    updated_at: row.updated_at,
                    anchor_detached: row.anchor_detached,
                };
                let messages = messages_by_thread
                    .remove(&row.thread_id)
                    .unwrap_or_default();
                row.into_thread(messages)
            })
            .collect()
    }

    /// Append a reply and optionally move an `open` thread to `addressed`.
    pub async fn reply_comment_thread(
        &self,
        doc_id: &DocId,
        thread_id: &str,
        reply: NewCommentReply,
    ) -> Result<CommentThread, StoreError> {
        validate_comment_body(&reply.body)?;
        if reply.mark_addressed && reply.author_type != CommentAuthorType::Agent {
            return Err(StoreError::InvalidRequest(
                "only agents may mark a comment addressed".into(),
            ));
        }
        let mut tx = self.pool.begin().await?;
        let current_rev_row = sqlx::query!(
            "SELECT current_rev FROM report_documents WHERE doc_id = $1",
            doc_id.as_str()
        )
        .fetch_optional(&mut *tx)
        .await?
        .ok_or_else(|| StoreError::NotFound {
            doc_id: doc_id.clone(),
        })?;
        let current_rev = Revision::new(current_rev_row.current_rev)?;
        let mut thread = self
            .load_comment_thread_in_tx(&mut tx, doc_id, thread_id)
            .await?;
        let now = Utc::now();
        let message_id = short_id("msg");
        sqlx::query!(
            r#"INSERT INTO report_comment_messages
               (message_id, doc_id, thread_id, author_type, author, body, created_at)
               VALUES ($1, $2, $3, $4, $5, $6, $7)"#,
            &message_id,
            doc_id.as_str(),
            thread_id,
            reply.author_type.as_str(),
            &reply.author,
            &reply.body,
            now,
        )
        .execute(&mut *tx)
        .await?;

        let mut status = thread.status;
        let mut addressed_rev = thread.addressed_rev;
        if reply.mark_addressed && status == CommentStatus::Open {
            let revision = reply.addressed_rev.unwrap_or(current_rev);
            if revision > current_rev {
                return Err(StoreError::InvalidRequest(format!(
                    "addressed_rev {} is ahead of current revision {}",
                    revision.get(),
                    current_rev.get()
                )));
            }
            status = CommentStatus::Addressed;
            addressed_rev = Some(revision);
        }
        sqlx::query!(
            r#"UPDATE report_comment_threads
               SET status = $3, addressed_rev = $4, updated_at = $5
               WHERE doc_id = $1 AND thread_id = $2"#,
            doc_id.as_str(),
            thread_id,
            status.as_str(),
            addressed_rev.map(Revision::get),
            now,
        )
        .execute(&mut *tx)
        .await?;
        tx.commit().await?;

        thread.status = status;
        thread.addressed_rev = addressed_rev;
        thread.updated_at = now;
        thread.messages.push(CommentMessage {
            message_id,
            thread_id: thread_id.to_string(),
            author_type: reply.author_type,
            author: reply.author,
            body: reply.body,
            created_at: now,
        });
        Ok(thread)
    }

    /// Delete a comment thread and its messages. Side-channel data never participates in revisions.
    pub async fn delete_comment_thread(
        &self,
        doc_id: &DocId,
        thread_id: &str,
    ) -> Result<(), StoreError> {
        let deleted = sqlx::query_scalar::<_, String>(
            r#"DELETE FROM report_comment_threads
               WHERE doc_id = $1 AND thread_id = $2
               RETURNING thread_id"#,
        )
        .bind(doc_id.as_str())
        .bind(thread_id)
        .fetch_optional(&self.pool)
        .await?;
        if deleted.is_none() {
            return Err(StoreError::CommentThreadNotFound {
                doc_id: doc_id.clone(),
                thread_id: thread_id.to_string(),
            });
        }
        Ok(())
    }

    /// Human-only status transition. Only `archived` and `open` are valid here.
    pub async fn update_comment_status(
        &self,
        doc_id: &DocId,
        thread_id: &str,
        status: CommentStatus,
    ) -> Result<CommentThread, StoreError> {
        if !matches!(status, CommentStatus::Open | CommentStatus::Archived) {
            return Err(StoreError::InvalidRequest(
                "REST may only archive a thread or reopen it".into(),
            ));
        }
        let mut tx = self.pool.begin().await?;
        let mut thread = self
            .load_comment_thread_in_tx(&mut tx, doc_id, thread_id)
            .await?;
        let now = Utc::now();
        let addressed_rev = if status == CommentStatus::Open {
            None
        } else {
            thread.addressed_rev
        };
        sqlx::query!(
            r#"UPDATE report_comment_threads
               SET status = $3, addressed_rev = $4, updated_at = $5
               WHERE doc_id = $1 AND thread_id = $2"#,
            doc_id.as_str(),
            thread_id,
            status.as_str(),
            addressed_rev.map(Revision::get),
            now,
        )
        .execute(&mut *tx)
        .await?;
        tx.commit().await?;
        thread.status = status;
        thread.addressed_rev = addressed_rev;
        thread.updated_at = now;
        Ok(thread)
    }

    async fn load_comment_thread_in_tx(
        &self,
        tx: &mut Transaction<'_, Postgres>,
        doc_id: &DocId,
        thread_id: &str,
    ) -> Result<CommentThread, StoreError> {
        let row = sqlx::query!(
            r#"SELECT t.doc_id, t.thread_id, t.block_id, t.end_block_id, t.anchor_blocks,
                      t.section_id, t.anchor_snapshot, t.quote, t.end_quote, t.status,
                      t.created_rev, t.addressed_rev, t.created_at, t.updated_at,
                      (NOT EXISTS (
                          SELECT 1 FROM report_blocks b
                          WHERE b.doc_id = t.doc_id
                            AND (b.block_id = t.block_id
                                 OR b.block_id = t.end_block_id
                                 OR b.block_id IN (
                                     SELECT jsonb_array_elements_text(t.anchor_blocks)
                                 ))
                      )) AS "anchor_detached!"
               FROM report_comment_threads t
               WHERE t.doc_id = $1 AND t.thread_id = $2"#,
            doc_id.as_str(),
            thread_id,
        )
        .fetch_optional(&mut **tx)
        .await?
        .ok_or_else(|| StoreError::CommentThreadNotFound {
            doc_id: doc_id.clone(),
            thread_id: thread_id.to_string(),
        })?;
        let message_rows = sqlx::query!(
            r#"SELECT message_id, thread_id, author_type, author, body, created_at
               FROM report_comment_messages
               WHERE doc_id = $1 AND thread_id = $2
               ORDER BY created_at ASC, message_id ASC"#,
            doc_id.as_str(),
            thread_id,
        )
        .fetch_all(&mut **tx)
        .await?;
        let messages = message_rows
            .into_iter()
            .map(|row| {
                build_comment_message(
                    row.message_id,
                    row.thread_id,
                    &row.author_type,
                    row.author,
                    row.body,
                    row.created_at,
                )
            })
            .collect::<Result<Vec<_>, _>>()?;
        CommentThreadRow {
            doc_id: row.doc_id,
            thread_id: row.thread_id,
            block_id: row.block_id,
            end_block_id: row.end_block_id,
            anchor_blocks: row.anchor_blocks,
            section_id: row.section_id,
            anchor_snapshot: row.anchor_snapshot,
            quote: row.quote,
            end_quote: row.end_quote,
            status: row.status,
            created_rev: row.created_rev,
            addressed_rev: row.addressed_rev,
            created_at: row.created_at,
            updated_at: row.updated_at,
            anchor_detached: row.anchor_detached,
        }
        .into_thread(messages)
    }
}

impl CommentThreadRow {
    fn into_thread(self, messages: Vec<CommentMessage>) -> Result<CommentThread, StoreError> {
        Ok(CommentThread {
            thread_id: self.thread_id,
            doc_id: DocId::parse(self.doc_id)?,
            block_id: BlockId::parse(self.block_id)?,
            end_block_id: self.end_block_id.map(BlockId::parse).transpose()?,
            anchor_blocks: parse_json(self.anchor_blocks, "report_comment_threads.anchor_blocks")?,
            section_id: self.section_id.map(BlockId::parse).transpose()?,
            anchor_snapshot: self.anchor_snapshot,
            quote: self
                .quote
                .map(|value| parse_json(value, "report_comment_threads.quote"))
                .transpose()?,
            end_quote: self
                .end_quote
                .map(|value| parse_json(value, "report_comment_threads.end_quote"))
                .transpose()?,
            status: CommentStatus::parse(&self.status).ok_or_else(|| {
                StoreError::Internal(format!(
                    "invalid comment status `{}` persisted in database",
                    self.status
                ))
            })?,
            created_rev: Revision::new(self.created_rev)?,
            addressed_rev: self.addressed_rev.map(Revision::new).transpose()?,
            anchor_detached: self.anchor_detached,
            messages,
            created_at: self.created_at,
            updated_at: self.updated_at,
        })
    }
}

fn build_comment_message(
    message_id: String,
    thread_id: String,
    author_type: &str,
    author: String,
    body: String,
    created_at: DateTime<Utc>,
) -> Result<CommentMessage, StoreError> {
    let author_type = CommentAuthorType::parse(author_type).ok_or_else(|| {
        StoreError::Internal(format!(
            "invalid comment author type `{author_type}` persisted in database"
        ))
    })?;
    Ok(CommentMessage {
        message_id,
        thread_id,
        author_type,
        author,
        body,
        created_at,
    })
}

fn comment_section_for_index(document: &ReportDocument, index: usize) -> Option<BlockId> {
    let block = document.blocks.get(index)?;
    if matches!(block.body, BlockKind::Heading { .. }) {
        return Some(block.id.clone());
    }
    document
        .blocks
        .iter()
        .take(index)
        .rev()
        .find(|candidate| matches!(candidate.body, BlockKind::Heading { .. }))
        .map(|heading| heading.id.clone())
}

/// Ensure the start/end blocks are included and return every surviving anchor in document order.
fn normalize_anchor_blocks(
    document: &ReportDocument,
    start: &BlockId,
    end: Option<&BlockId>,
    requested: &[BlockId],
) -> Vec<BlockId> {
    let mut selected: HashSet<&BlockId> = requested.iter().collect();
    selected.insert(start);
    if let Some(end) = end {
        selected.insert(end);
    }
    document
        .blocks
        .iter()
        .filter(|block| selected.contains(&block.id))
        .map(|block| block.id.clone())
        .collect()
}

fn comment_anchor_snapshot(block: &Block) -> Option<String> {
    let text = block.visible_text();
    let text = text.trim();
    if text.is_empty() {
        return None;
    }
    let mut chars = text.chars();
    let mut snapshot: String = chars.by_ref().take(160).collect();
    if chars.next().is_some() {
        snapshot.push('…');
    }
    Some(snapshot)
}

fn validate_comment_body(body: &str) -> Result<(), StoreError> {
    if body.trim().is_empty() {
        return Err(StoreError::InvalidRequest(
            "comment body must not be empty".into(),
        ));
    }
    if body.chars().count() > 20_000 {
        return Err(StoreError::InvalidRequest(
            "comment body must be at most 20000 characters".into(),
        ));
    }
    Ok(())
}
