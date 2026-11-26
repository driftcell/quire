use super::*;

impl QuireReportService {
    // ---------------------------------------------------------------------
    // Digest helpers
    // ---------------------------------------------------------------------
    fn block_digest(block: &Block) -> BlockDigest {
        let is_heading = matches!(&block.body, BlockKind::Heading { .. });
        let char_count = block.char_count();
        BlockDigest {
            id: block.id.to_string(),
            block_type: block.block_type().to_string(),
            level: match &block.body {
                BlockKind::Heading { level, .. } => Some(*level),
                _ => None,
            },
            preview: (char_count > 0).then(|| block.preview_30()),
            char_count: (!is_heading && char_count > 0).then_some(char_count),
            source_ids: block.source_ids.iter().map(ToString::to_string).collect(),
        }
    }

    /// Build the compact outline returned by `get_document_digest` and create receipts.
    pub(crate) fn build_digest(
        document: &ReportDocument,
        query: Option<&str>,
        fallback_target: Option<NonZeroU32>,
    ) -> DocumentDigest {
        let query = query
            .map(str::trim)
            .filter(|query| !query.is_empty())
            .map(str::to_lowercase);
        let blocks = document
            .blocks
            .iter()
            .filter(|block| {
                let Some(query) = &query else {
                    return true;
                };
                block.id.to_lowercase().contains(query)
                    || block.block_type().contains(query.as_str())
                    || block.visible_text().to_lowercase().contains(query)
            })
            .map(Self::block_digest)
            .collect::<Vec<_>>();
        let paragraph_count = document
            .blocks
            .iter()
            .filter(|block| matches!(&block.body, BlockKind::Paragraph { .. }))
            .count();
        let prose_char_count: usize = document
            .blocks
            .iter()
            .filter(|block| matches!(&block.body, BlockKind::Paragraph { .. }))
            .map(|block| block.char_count())
            .sum();
        let visual_block_count = document
            .blocks
            .iter()
            .filter(|block| {
                matches!(
                    &block.body,
                    BlockKind::Table(_) | BlockKind::Echarts(_) | BlockKind::CustomHtml(_)
                )
            })
            .count();
        let cited_source_count = scan_document_citations(document).cited_source_ids.len();
        let target_prose_chars = document.target_prose_chars.or(fallback_target);
        DocumentDigest {
            title: document.title(),
            current_rev: document.revision.get(),
            theme: document.theme.summary(),
            toc: document.toc.clone(),
            block_count: document.blocks.len(),
            source_count: document.sources.len(),
            paragraph_count,
            prose_char_count,
            target_prose_chars,
            prose_char_delta: target_prose_chars.map(|target| {
                i64::try_from(prose_char_count).unwrap_or(i64::MAX) - i64::from(target.get())
            }),
            visual_block_count,
            cited_source_count,
            blocks,
        }
    }

    pub async fn exec_create_report(
        &self,
        context: RequestContext,
        request: CreateReportParams,
    ) -> Result<CreateReportReceipt, ErrorData> {
        if context.preview_doc_id.is_some() {
            return Err(mcp_error(StoreError::Forbidden(
                "preview token cannot create reports".into(),
            )));
        }
        let title = request.title.trim();
        if title.is_empty() {
            return Err(invalid_params(
                "title must not be empty",
                "Provide a short human-readable report title.",
                None,
            ));
        }
        if let Some(key) = request.idempotency_key.as_deref()
            && (key.trim().is_empty() || key.len() > 128)
        {
            return Err(invalid_params(
                "idempotency_key must be 1-128 characters",
                "Use a stable key such as `sess_01:create_report`.",
                None,
            ));
        }
        let session_id = match (
            context.session_id.as_ref(),
            self.state.harness_bridge.as_ref(),
        ) {
            (requested, Some(bridge)) => bridge
                .prepare_session_for_report(
                    requested,
                    context.user_id.as_deref(),
                    context.is_admin,
                    title,
                )
                .await
                .map_err(|error| mcp_error(StoreError::Forbidden(error)))?,
            (Some(session_id), None) => session_id.clone(),
            (None, None) => {
                return Err(invalid_params(
                    "create_report requires a session identity",
                    "Send X-Session-Id or per-call _meta.quire.session_id.",
                    None,
                ));
            }
        };
        let report_template = if let Some(bridge) = self.state.harness_bridge.as_ref() {
            bridge
                .resolve_report_template(
                    request.theme_preset.as_deref(),
                    context.user_id.as_deref(),
                    context.is_admin,
                )
                .await
                .map_err(|error| mcp_error(StoreError::Forbidden(error)))?
        } else {
            None
        };
        let actor = context.actor;
        let scoped_idempotency_key = request.idempotency_key.as_deref().map(|key| {
            let trimmed = key.trim();
            let prefix = format!("{}:", session_id.as_str());
            let scoped = if trimmed.starts_with(&prefix) {
                trimmed.to_string()
            } else {
                format!("{prefix}{trimmed}")
            };
            if scoped.len() > 128 {
                scoped.chars().take(128).collect()
            } else {
                scoped
            }
        });
        let doc_id = match request
            .doc_id
            .as_deref()
            .map(str::trim)
            .filter(|value| !value.is_empty())
        {
            Some(value) => {
                let requested_id = DocId::parse(value).map_err(protocol_error)?;
                match self.state.store.get_document(&requested_id).await {
                    Ok(existing) if existing.session_id.as_ref() != Some(&session_id) => {
                        let suffix = short_token(6);
                        let base: String = requested_id.as_str().chars().take(56).collect();
                        DocId::parse(format!("{base}-{suffix}")).map_err(protocol_error)?
                    }
                    _ => requested_id,
                }
            }
            None => generate_doc_id(title)?,
        };

        let mut document = ReportDocument::new(doc_id);
        if let Some(template) = report_template {
            document.theme = template.theme;
            document.cover = template.cover;
            document.toc = template.toc;
            document.references = template.references;
        }
        document.target_prose_chars = request.target_prose_chars;
        document.session_id = Some(session_id.clone());
        document.cover.enabled = request.enable_cover.unwrap_or(true);
        document.cover.title = title.to_string();
        if let Some(subtitle) = request
            .subtitle
            .as_deref()
            .map(str::trim)
            .filter(|s| !s.is_empty())
        {
            document.cover.subtitle = Some(subtitle.to_string());
        }
        if let Some(authors) = request.authors {
            document.cover.authors = authors
                .into_iter()
                .map(|a| a.trim().to_string())
                .filter(|a| !a.is_empty())
                .collect();
        }
        if let Some(date) = request
            .date
            .as_deref()
            .map(str::trim)
            .filter(|s| !s.is_empty())
        {
            document.cover.date = Some(date.to_string());
        }
        if let Some(meta) = request.cover_meta {
            document.cover.meta = meta;
        }
        document.toc.enabled = request.enable_toc.unwrap_or(true);
        if title.chars().any(is_cjk_char) {
            document.toc.title = "目录".to_string();
            document.references.title = "参考文献".to_string();
        }
        if let Some(preset) = request.theme_preset {
            document.theme.preset = preset;
        }
        document.blocks = request.initial_blocks.unwrap_or_default();
        let duplicates = document.duplicate_block_ids();
        if !duplicates.is_empty() {
            return Err(invalid_params(
                format!("duplicate initial block IDs: {}", duplicates.join(", ")),
                "Every block id must be unique. Rename duplicates before create_report.",
                Some(McpErrorDetails::DuplicateBlockIds {
                    duplicate_block_ids: duplicates,
                }),
            ));
        }

        let result = self
            .state
            .store
            .create_document(&document, &actor, scoped_idempotency_key.as_deref())
            .await
            .map_err(mcp_error)?;
        if let Some(bridge) = self.state.harness_bridge.as_ref() {
            let effective_session = result.session_id.as_ref().unwrap_or(&session_id);
            bridge
                .bind_created_report(
                    effective_session,
                    context.user_id.as_deref(),
                    context.is_admin,
                    &result.title,
                    &result.doc_id,
                )
                .await
                .map_err(|error| mcp_error(StoreError::Forbidden(error)))?;
        }
        let stored = self
            .state
            .store
            .get_document(&result.doc_id)
            .await
            .map_err(mcp_error)?;
        let digest = Self::build_digest(&stored, None, None);
        let revision = result.revision.get();
        let _next_hint = if result.idempotent_replay {
            format!(
                "Idempotent replay. Read get_document_digest or call apply_operations with expected_revision={}.",
                revision
            )
        } else {
            "Call get_protocol_guide(\"tables_and_charts\") and get_document_digest before authoring. Sub-agents should own disjoint block_id prefixes.".to_string()
        };
        Ok(CreateReportReceipt {
            doc_id: result.doc_id.to_string(),
            revision,
            title: result.title,
            session_id: result
                .session_id
                .map(|session| session.as_str().to_string()),
            idempotent_replay: result.idempotent_replay,
            digest,
            _next_hint,
        })
    }
}
