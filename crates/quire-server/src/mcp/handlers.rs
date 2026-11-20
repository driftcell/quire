//! MCP tool implementations, Smart CAS reload helpers and draft ingestion utilities.

use std::{
    collections::{BTreeMap, BTreeSet, HashSet},
    path::Path,
};

use axum::http::request::Parts;
use indexmap::IndexMap;
use quire_protocol::{
    Block, BlockId, BlockKind, COVER_VARIANTS, CommentAuthorType, CommentStatus, CoverConfig,
    Diagnostic, DocId, EphemeralClearEvent, EphemeralPayload, IdAssignment, MarkdownOptions,
    MarkdownProjector, PageChromeConfig, PatchOp, ProtocolError, ReportDocument, ReportDraft,
    Revision, Severity, SourceEntry, SourceId, TOC_VARIANTS, is_safe_css_token_value,
    is_valid_css_token_name, lint_document, short_token,
};
use quire_store::{
    ApplyResult, CommitExtras, NewCommentReply, NewCommentThread, SourceUpsert, StoreError,
    merge_source_entries, normalized_source_key,
};
use rmcp::{
    ErrorData,
    handler::server::{
        tool::Extension,
        wrapper::{Json, Parameters},
    },
    tool, tool_router,
};
use serde_json::{Value, json};

use crate::{
    auth::RequestContext,
    config::Config,
    error::{invalid_params, mcp_error},
    guide::{guide, guide_workflow},
    state::AppState,
};

use super::QuireMcpServer;
use super::schema::*;

#[tool_router(router = tool_router, vis = "pub(crate)")]
impl QuireMcpServer {
    // ---------------------------------------------------------------------
    // Tool 1: protocol guide
    // ---------------------------------------------------------------------
    #[tool(
        name = "get_protocol_guide",
        description = "Progressive protocol schema disclosure. Call quickstart first, then tables_and_charts, citations_and_css and parallel_workflow as needed."
    )]
    pub async fn get_protocol_guide(
        &self,
        Parameters(request): Parameters<GuideParams>,
    ) -> Result<Json<GuideReceipt>, ErrorData> {
        let receipt = guide(request.topic);
        let workflow = guide_workflow();
        Ok(Json(GuideReceipt {
            topic: receipt.topic,
            guide: receipt.guide,
            recommended_tools: receipt.recommended_tools,
            workflow,
            _next_hint: receipt._next_hint,
        }))
    }

    // ---------------------------------------------------------------------
    // Tool 2: create report
    // ---------------------------------------------------------------------
    #[tool(
        name = "create_report",
        description = "Create a new report. Uses X-Session-Id from the MCP HTTP header for frontend session discovery. Idempotent when idempotency_key is supplied."
    )]
    pub async fn create_report(
        &self,
        Extension(parts): Extension<Parts>,
        Parameters(request): Parameters<CreateReportParams>,
    ) -> Result<Json<CreateReportReceipt>, ErrorData> {
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
        let context = RequestContext::from_parts(&parts).map_err(|error| {
            invalid_params(
                error.to_string(),
                "Use 1-64 letters/digits plus '-' '_' '.' in X-Session-Id and a short X-Actor.",
                None,
            )
        })?;
        let session_id = context.session_id;
        let actor = context.actor;
        let doc_id = match request
            .doc_id
            .as_deref()
            .map(str::trim)
            .filter(|value| !value.is_empty())
        {
            Some(value) => DocId::parse(value).map_err(protocol_error)?,
            None => generate_doc_id(title)?,
        };

        let mut document = ReportDocument::new(doc_id);
        document.session_id = session_id;
        document.cover.enabled = request.enable_cover.unwrap_or(true);
        document.cover.title = title.to_string();
        document.toc.enabled = request.enable_toc.unwrap_or(true);
        if let Some(preset) = request.theme_preset {
            document.theme.preset = preset;
        }
        document.blocks = request.initial_blocks.unwrap_or_default();
        let duplicates = document.duplicate_block_ids();
        if !duplicates.is_empty() {
            return Err(invalid_params(
                format!("duplicate initial block IDs: {}", duplicates.join(", ")),
                "Every block id must be unique. Rename duplicates before create_report.",
                Some(json!({"duplicate_block_ids": duplicates})),
            ));
        }

        let result = self
            .state
            .store
            .create_document(&document, &actor, request.idempotency_key.as_deref())
            .await
            .map_err(mcp_error)?;
        let stored = self
            .state
            .store
            .get_document(&result.doc_id)
            .await
            .map_err(mcp_error)?;
        let digest = build_digest(&stored, None);
        let revision = result.revision.get();
        let _next_hint = if result.idempotent_replay {
            format!(
                "Idempotent replay. Read get_document_digest or call apply_operations with expected_revision={}.",
                revision
            )
        } else {
            "Call get_protocol_guide(\"tables_and_charts\") and get_document_digest before authoring. Sub-agents should own disjoint block_id prefixes.".to_string()
        };
        Ok(Json(CreateReportReceipt {
            doc_id: result.doc_id.to_string(),
            revision,
            title: result.title,
            session_id: result
                .session_id
                .map(|session| session.as_str().to_string()),
            idempotent_replay: result.idempotent_replay,
            digest,
            _next_hint,
        }))
    }

    // ---------------------------------------------------------------------
    // Tool 3: list reports
    // ---------------------------------------------------------------------
    #[tool(
        name = "list_reports",
        description = "List reports, newest first, optionally filtered by doc_id or title substring."
    )]
    pub async fn list_reports(
        &self,
        Parameters(request): Parameters<ListReportsParams>,
    ) -> Result<Json<ListReportsReceipt>, ErrorData> {
        let list = self
            .state
            .store
            .list_reports(request.query.as_deref(), request.limit.unwrap_or(20), 0)
            .await
            .map_err(mcp_error)?;
        let _next_hint = if list.reports.is_empty() {
            "No reports matched. Call create_report to start one.".to_string()
        } else {
            "Call get_document_digest(doc_id) for the report you want to edit.".to_string()
        };
        let reports = list
            .reports
            .into_iter()
            .map(ReportSummaryView::from)
            .collect();
        Ok(Json(ListReportsReceipt {
            reports,
            total: list.total,
            limit: list.limit,
            offset: list.offset,
            _next_hint,
        }))
    }

    // ---------------------------------------------------------------------
    // Tool 4: apply draft
    // ---------------------------------------------------------------------
    #[tool(
        name = "apply_draft",
        description = "Ingest an offline draft.json. Auto-prefixes colliding block IDs, deduplicates sources by URL/title, and merges atomically in create_or_replace, append_section or replace_range mode."
    )]
    pub async fn apply_draft(
        &self,
        Extension(parts): Extension<Parts>,
        Parameters(request): Parameters<ApplyDraftParams>,
    ) -> Result<Json<ApplyDraftReceipt>, ErrorData> {
        let draft = self.read_draft(&request.draft_file_path).await?;
        let actor = RequestContext::from_parts(&parts)
            .map_err(|error| {
                invalid_params(error.to_string(), "Check X-Session-Id and X-Actor.", None)
            })?
            .actor;
        let preview_ids = normalize_preview_ids(request.preview_ids)?;
        let expected_revision = request.expected_revision.map(parse_revision).transpose()?;
        match request.mode {
            DraftMode::CreateOrReplace if request.target_doc_id.is_none() => {
                self.create_from_draft(draft, request.sources, &parts, &actor, &preview_ids)
                    .await
            }
            DraftMode::CreateOrReplace => {
                let target = parse_doc_id(request.target_doc_id.as_deref().ok_or_else(|| {
                    invalid_params(
                        "target_doc_id is required for replace mode",
                        "Pass the existing doc_id returned by create_report.",
                        None,
                    )
                })?)?;
                let mut attempt = 0;
                loop {
                    match self
                        .replace_with_draft(
                            &target,
                            draft.clone(),
                            request.sources.clone(),
                            &actor,
                            expected_revision,
                            &preview_ids,
                        )
                        .await
                    {
                        Ok(receipt) => break Ok(receipt),
                        Err(error)
                            if expected_revision.is_none()
                                && is_retryable_conflict(&error)
                                && attempt < 3 =>
                        {
                            attempt += 1;
                            continue;
                        }
                        Err(error) => break Err(error),
                    }
                }
            }
            DraftMode::AppendSection => {
                let target = parse_doc_id(request.target_doc_id.as_deref().ok_or_else(|| {
                    invalid_params(
                        "target_doc_id is required for append_section mode",
                        "Pass the existing doc_id returned by create_report.",
                        None,
                    )
                })?)?;
                let mut attempt = 0;
                loop {
                    match self
                        .append_draft(
                            &target,
                            draft.clone(),
                            request.sources.clone(),
                            &actor,
                            expected_revision,
                            &preview_ids,
                        )
                        .await
                    {
                        Ok(receipt) => break Ok(receipt),
                        Err(error)
                            if expected_revision.is_none()
                                && is_retryable_conflict(&error)
                                && attempt < 3 =>
                        {
                            attempt += 1;
                            continue;
                        }
                        Err(error) => break Err(error),
                    }
                }
            }
            DraftMode::ReplaceRange => {
                let target = parse_doc_id(request.target_doc_id.as_deref().ok_or_else(|| {
                    invalid_params(
                        "target_doc_id is required for replace_range mode",
                        "Pass the existing doc_id and both range endpoints.",
                        None,
                    )
                })?)?;
                let start = parse_block_id(&request.start_block_id.ok_or_else(|| {
                    invalid_params(
                        "start_block_id is required for replace_range",
                        "Read get_document_digest and pass the first block ID to replace.",
                        None,
                    )
                })?)?;
                let end = parse_block_id(&request.end_block_id.ok_or_else(|| {
                    invalid_params(
                        "end_block_id is required for replace_range",
                        "Read get_document_digest and pass the last block ID to replace.",
                        None,
                    )
                })?)?;
                let mut attempt = 0;
                loop {
                    match self
                        .replace_range_with_draft(
                            &target,
                            draft.clone(),
                            &start,
                            &end,
                            request.sources.clone(),
                            &actor,
                            expected_revision,
                            &preview_ids,
                        )
                        .await
                    {
                        Ok(receipt) => break Ok(receipt),
                        Err(error)
                            if expected_revision.is_none()
                                && is_retryable_conflict(&error)
                                && attempt < 3 =>
                        {
                            attempt += 1;
                            continue;
                        }
                        Err(error) => break Err(error),
                    }
                }
            }
        }
    }

    // ---------------------------------------------------------------------
    // Tool 5: document digest
    // ---------------------------------------------------------------------
    #[tool(
        name = "get_document_digest",
        description = "Low-token outline: current_rev, theme summary and compact blocks (id/type/level/preview/char_count/source_ids; empty fields omitted). Optional keyword query filters blocks."
    )]
    pub async fn get_document_digest(
        &self,
        Parameters(request): Parameters<DigestParams>,
    ) -> Result<Json<DigestReceipt>, ErrorData> {
        let doc_id = parse_doc_id(&request.doc_id)?;
        let document = self
            .state
            .store
            .get_document(&doc_id)
            .await
            .map_err(mcp_error)?;
        let digest = build_digest(&document, request.query.as_deref());
        let _next_hint = if digest.blocks.is_empty() {
            "No blocks matched. Append blocks with apply_operations.".to_string()
        } else {
            format!(
                "rev={}; read_blocks/read_as_markdown for details.",
                digest.current_rev
            )
        };
        Ok(Json(DigestReceipt { digest, _next_hint }))
    }

    // ---------------------------------------------------------------------
    // Tool 6: read as markdown
    // ---------------------------------------------------------------------
    #[tool(
        name = "read_as_markdown",
        description = "Project an online report or offline draft.json into token-efficient Markdown with block anchors, flattened tables, expanded chart data and expanded citations."
    )]
    pub async fn read_as_markdown(
        &self,
        Parameters(request): Parameters<ReadMarkdownParams>,
    ) -> Result<Json<ReadMarkdownReceipt>, ErrorData> {
        let markdown_options = MarkdownOptions {
            include_chart_data: request.include_chart_data.unwrap_or(true),
            include_front_matter: true,
        };
        let requested_doc_id = request.doc_id.as_deref().map(parse_doc_id).transpose()?;
        let (source, document) = match (
            requested_doc_id.as_ref(),
            request.draft_file_path.as_deref(),
        ) {
            (Some(doc_id), None) => (
                format!("doc:{doc_id}"),
                self.state
                    .store
                    .get_document(doc_id)
                    .await
                    .map_err(mcp_error)?,
            ),
            (None, Some(path)) => {
                let draft = self.read_draft(path).await?;
                (
                    format!("draft:{path}"),
                    draft_to_document(draft, "draft-preview")?,
                )
            }
            (Some(_), Some(_)) => {
                return Err(invalid_params(
                    "provide either doc_id or draft_file_path, not both",
                    "Use doc_id for online reports and draft_file_path for offline drafts.",
                    None,
                ));
            }
            (None, None) => {
                return Err(invalid_params(
                    "doc_id or draft_file_path is required",
                    "Pass doc_id for an online report or a workspace-relative draft path.",
                    None,
                ));
            }
        };
        let projector = MarkdownProjector::new(&document, &markdown_options);
        let markdown = if let Some(block_ids) = &request.block_ids {
            let block_ids = parse_block_ids(block_ids)?;
            projector.render_blocks(&block_ids).map_err(|error| {
                invalid_params(
                    error.to_string(),
                    "Use valid block IDs from get_document_digest.",
                    None,
                )
            })?
        } else if let Some(section) = &request.section_heading {
            projector.render_section(section).map_err(|error| {
                invalid_params(
                    error.to_string(),
                    "Use an exact or substring heading from get_document_digest.",
                    None,
                )
            })?
        } else {
            projector.render().map_err(|error| {
                invalid_params(error.to_string(), "Reload the document and retry.", None)
            })?
        };
        let _next_hint = if request.draft_file_path.is_some() {
            "If the projection looks right, merge with apply_draft(draft_file_path=..., target_doc_id=...).".to_string()
        } else {
            "Patch only blocks that need changes; use the block anchors above as exact IDs."
                .to_string()
        };
        Ok(Json(ReadMarkdownReceipt {
            source,
            doc_id: request
                .draft_file_path
                .is_none()
                .then(|| document.doc_id.to_string()),
            revision: document.revision.get(),
            markdown_chars: markdown.chars().count(),
            markdown,
            _next_hint,
        }))
    }

    // ---------------------------------------------------------------------
    // Tool 7: read blocks
    // ---------------------------------------------------------------------
    #[tool(
        name = "read_blocks",
        description = "Fetch full canonical protocol JSON for targeted blocks."
    )]
    pub async fn read_blocks(
        &self,
        Parameters(request): Parameters<ReadBlocksParams>,
    ) -> Result<Json<ReadBlocksReceipt>, ErrorData> {
        let doc_id = parse_doc_id(&request.doc_id)?;
        let block_ids = parse_block_ids(&request.block_ids)?;
        let document = self
            .state
            .store
            .get_document(&doc_id)
            .await
            .map_err(mcp_error)?;
        let mut blocks = Vec::new();
        let mut missing = Vec::new();
        for block_id in &block_ids {
            match document.block(block_id.as_str()) {
                Some(block) => blocks.push(block.clone()),
                None => missing.push(block_id.to_string()),
            }
        }
        if !missing.is_empty() {
            let valid: Vec<&str> = document
                .blocks
                .iter()
                .map(|block| block.id.as_str())
                .collect();
            return Err(invalid_params(
                format!("block IDs not found: {}", missing.join(", ")),
                format!("Valid block_ids: {}", valid.join(", ")),
                Some(json!({"missing": missing, "valid_block_ids": valid})),
            ));
        }
        Ok(Json(ReadBlocksReceipt {
            doc_id: document.doc_id.to_string(),
            revision: document.revision.get(),
            blocks,
            _next_hint: "Patch specific fields with apply_operations using the current revision."
                .into(),
        }))
    }

    // ---------------------------------------------------------------------
    // Tool 8: apply operations
    // ---------------------------------------------------------------------
    #[tool(
        name = "apply_operations",
        description = "Atomically apply declarative block patch operations with Smart CAS rebasing and optional same-transaction source upserts. Returns new_revision, assigned_ids, inline_warnings and _next_hint."
    )]
    pub async fn apply_operations(
        &self,
        Extension(parts): Extension<Parts>,
        Parameters(request): Parameters<ApplyOperationsParams>,
    ) -> Result<Json<ApplyOperationsReceipt>, ErrorData> {
        if request.summary.trim().is_empty() {
            return Err(invalid_params(
                "summary must not be empty",
                "Describe the intent, for example `write chapter 2 executive summary`.",
                None,
            ));
        }
        let actor = request_context(&parts)?.actor;
        let doc_id = parse_doc_id(&request.doc_id)?;
        let expected_revision = parse_revision(request.expected_revision)?;
        let source_upserts = inline_source_upserts(request.sources)?;
        let preview_ids = normalize_preview_ids(request.preview_ids)?;
        let result = self
            .state
            .store
            .apply_operations_with_extras(
                &doc_id,
                expected_revision,
                &actor,
                request.summary.trim(),
                &request.ops,
                CommitExtras {
                    sources: source_upserts,
                    cleared_preview_ids: preview_ids.clone(),
                },
            )
            .await
            .map_err(mcp_error)?;
        if result.outcome.no_op {
            publish_ephemeral_clear(&self.state, &doc_id, &preview_ids).await;
        }
        let touched = touched_blocks(&request.ops, &result.outcome.assigned_ids);
        let (blocking_errors, inline_warnings) =
            split_scoped_diagnostics(result.inline_warnings, &touched);
        let _next_hint = if !blocking_errors.is_empty() {
            format!(
                "Committed as revision {} with {} blocking error(s). Fix `blocking_errors`, then lint again and continue at expected_revision={}.",
                result.new_revision,
                blocking_errors.len(),
                result.new_revision
            )
        } else if result.outcome.no_op {
            format!(
                "Batch was a no-op; no revision was created. Current revision is {}.",
                result.new_revision
            )
        } else if result.rebased {
            format!(
                "Committed as revision {} after rebasing over {} revision(s) from base {}. Continue at expected_revision={}.",
                result.new_revision, result.rebased_over, result.base_revision, result.new_revision
            )
        } else {
            format!(
                "Call lint_document, then continue at expected_revision={}.",
                result.new_revision
            )
        };
        Ok(Json(ApplyOperationsReceipt {
            new_revision: result.new_revision.get(),
            base_revision: result.base_revision.get(),
            rebased: result.rebased,
            rebased_over: result.rebased_over,
            committed: true,
            assigned_ids: result.outcome.assigned_ids,
            inline_warnings,
            blocking_errors,
            _next_hint,
        }))
    }

    // ---------------------------------------------------------------------
    // Tool 9: rollback
    // ---------------------------------------------------------------------
    #[tool(
        name = "rollback_revision",
        description = "Revert document state by replaying stored inverse_ops in reverse order. Creates a new monotonic revision; history is never rewritten."
    )]
    pub async fn rollback_revision(
        &self,
        Extension(parts): Extension<Parts>,
        Parameters(request): Parameters<RollbackParams>,
    ) -> Result<Json<RollbackReceipt>, ErrorData> {
        let actor = request_context(&parts)?.actor;
        let doc_id = parse_doc_id(&request.doc_id)?;
        let target_revision = parse_revision(request.target_revision)?;
        let result = self
            .state
            .store
            .rollback_to_revision(&doc_id, target_revision, &actor)
            .await
            .map_err(mcp_error)?;
        let summary = format!(
            "Rolled back to revision {} by replaying {} inverse revision(s); history is preserved as revision {}.",
            result.target_revision, result.replayed_revisions, result.new_revision
        );
        let _next_hint = format!(
            "Call get_document_digest or read_blocks to inspect revision {}, then lint_document before continuing edits.",
            result.new_revision
        );
        Ok(Json(RollbackReceipt {
            doc_id: doc_id.to_string(),
            new_revision: result.new_revision.get(),
            rolled_back_to: result.target_revision.get(),
            replayed_revisions: result.replayed_revisions,
            summary,
            _next_hint,
        }))
    }

    // ---------------------------------------------------------------------
    // Tool 10: upsert sources
    // ---------------------------------------------------------------------
    #[tool(
        name = "upsert_sources",
        description = "Register or update citation metadata with URL + normalized-title deduplication. Returns canonical_id_map; cite the canonical source_id."
    )]
    pub async fn upsert_sources(
        &self,
        Extension(parts): Extension<Parts>,
        Parameters(request): Parameters<UpsertSourcesParams>,
    ) -> Result<Json<UpsertSourcesReceipt>, ErrorData> {
        if request.sources.is_empty() {
            return Err(invalid_params(
                "sources must not be empty",
                "Supply at least one source object with id/title (and preferably url).",
                None,
            ));
        }
        let actor = request_context(&parts)?.actor;
        let doc_id = parse_doc_id(&request.doc_id)?;
        let upserts: Vec<SourceUpsert> = request
            .sources
            .into_iter()
            .map(|source| {
                let source_id = source.id.ok_or_else(|| {
                    invalid_params(
                        "source id is required",
                        "Every upsert_sources entry needs `id`, e.g. `\"id\":\"sec10\"`.",
                        None,
                    )
                })?;
                Ok(SourceUpsert {
                    source_id,
                    source: source.entry,
                })
            })
            .collect::<Result<Vec<_>, ErrorData>>()?;
        let result = self
            .state
            .store
            .upsert_sources(&doc_id, &actor, upserts)
            .await
            .map_err(mcp_error)?;
        let _next_hint = if result.changed {
            format!(
                "Sources committed at revision {}. Use canonical_id_map values in [^source_id] markers.",
                result.revision
            )
        } else {
            "Sources already existed; no revision was created. Use canonical_id_map when citing."
                .to_string()
        };
        Ok(Json(UpsertSourcesReceipt {
            doc_id: doc_id.to_string(),
            revision: result.revision.get(),
            changed: result.changed,
            canonical_id_map: result
                .canonical_id_map
                .into_iter()
                .map(|(from, to)| (from.to_string(), to.to_string()))
                .collect(),
            source_count: result.sources.len(),
            inline_warnings: result.inline_warnings,
            _next_hint,
        }))
    }

    // ---------------------------------------------------------------------
    // Tool 11: set cover
    // ---------------------------------------------------------------------
    #[tool(
        name = "set_cover",
        description = "Partially update cover fields (enabled, variant, title, subtitle, authors, date, meta, custom_html)."
    )]
    pub async fn set_cover(
        &self,
        Extension(parts): Extension<Parts>,
        Parameters(request): Parameters<SetCoverParams>,
    ) -> Result<Json<SetCoverReceipt>, ErrorData> {
        let actor = request_context(&parts)?.actor;
        let doc_id = parse_doc_id(&request.doc_id)?;
        let patch = request.cover_patch;
        let (result, updated) = self
            .apply_with_reload(&doc_id, &actor, "update_cover", |document| {
                let mut cover = document.cover.clone();
                patch.apply_to(&mut cover)?;
                Ok(vec![PatchOp::SetCover {
                    cover: cover.clone(),
                }])
            })
            .await?;
        let cover = updated.cover;
        let _next_hint = format!(
            "Cover committed at revision {}. Continue with configure_toc or authoring blocks.",
            result.new_revision
        );
        Ok(Json(SetCoverReceipt {
            doc_id: doc_id.to_string(),
            revision: result.new_revision.get(),
            cover,
            _next_hint,
        }))
    }

    // ---------------------------------------------------------------------
    // Tool 12: configure TOC
    // ---------------------------------------------------------------------
    #[tool(
        name = "configure_toc",
        description = "Enable/disable the auto-generated TOC and set title, max_depth (1..=3) and variant (standard|numbered|dotted)."
    )]
    pub async fn configure_toc(
        &self,
        Extension(parts): Extension<Parts>,
        Parameters(request): Parameters<ConfigureTocParams>,
    ) -> Result<Json<ConfigureTocReceipt>, ErrorData> {
        let actor = request_context(&parts)?.actor;
        if let Some(depth) = request.max_depth
            && !(1..=3).contains(&depth)
        {
            return Err(invalid_params(
                format!("max_depth {depth} is outside 1..=3"),
                "Use max_depth 1, 2 or 3.",
                Some(json!({"valid_max_depths":[1,2,3]})),
            ));
        }
        if let Some(variant) = &request.variant
            && !TOC_VARIANTS.contains(&variant.as_str())
        {
            return Err(invalid_params(
                format!("unknown TOC variant `{variant}`"),
                format!("Valid TOC variants: {}", TOC_VARIANTS.join(", ")),
                Some(json!({"valid_variants": TOC_VARIANTS})),
            ));
        }
        let doc_id = parse_doc_id(&request.doc_id)?;
        let (result, updated) = self
            .apply_with_reload(&doc_id, &actor, "configure_toc", |document| {
                let mut toc = document.toc.clone();
                toc.enabled = request.enabled;
                if let Some(title) = &request.title {
                    toc.title = title.clone();
                }
                if let Some(depth) = request.max_depth {
                    toc.max_depth = depth;
                }
                if let Some(variant) = &request.variant {
                    toc.variant = variant.clone();
                }
                Ok(vec![PatchOp::SetToc { toc: toc.clone() }])
            })
            .await?;
        let toc = updated.toc;
        let _next_hint = format!(
            "TOC committed at revision {}. Headings are auto-collected by the frontend; do not add TOC blocks.",
            result.new_revision
        );
        Ok(Json(ConfigureTocReceipt {
            doc_id: doc_id.to_string(),
            revision: result.new_revision.get(),
            toc,
            _next_hint,
        }))
    }

    // ---------------------------------------------------------------------
    // Tool 13: configure references
    // ---------------------------------------------------------------------
    #[tool(
        name = "configure_references",
        description = "Configure the bibliography: enabled, title, style (numeric|author_year) and page_break_before. Returns orphan (unreferenced) source IDs."
    )]
    pub async fn configure_references(
        &self,
        Extension(parts): Extension<Parts>,
        Parameters(request): Parameters<ConfigureReferencesParams>,
    ) -> Result<Json<ConfigureReferencesReceipt>, ErrorData> {
        let actor = request_context(&parts)?.actor;
        let doc_id = parse_doc_id(&request.doc_id)?;
        let (result, document) = self
            .apply_with_reload(&doc_id, &actor, "configure_references", |document| {
                let mut references = document.references.clone();
                references.enabled = request.enabled;
                if let Some(title) = &request.title {
                    references.title = title.clone();
                }
                if let Some(style) = request.style {
                    references.style = style;
                }
                if let Some(page_break) = request.page_break_before {
                    references.page_break_before = page_break;
                }
                Ok(vec![PatchOp::SetReferences {
                    references: references.clone(),
                }])
            })
            .await?;
        let references = document.references.clone();
        let scan = quire_protocol::scan_document_citations(&document);
        let cited: HashSet<&str> = scan
            .uses
            .iter()
            .map(|usage| usage.source_id.as_str())
            .collect();
        let orphan_sources: Vec<String> = document
            .sources
            .keys()
            .filter(|source_id| !cited.contains(source_id.as_str()))
            .map(ToString::to_string)
            .collect();
        let _next_hint = if orphan_sources.is_empty() {
            "References committed. Bibliography labels follow citation first-appearance order."
                .to_string()
        } else {
            format!(
                "References committed. Orphan source_ids (registered but uncited): {}. Cite or remove them.",
                orphan_sources.join(", ")
            )
        };
        Ok(Json(ConfigureReferencesReceipt {
            doc_id: doc_id.to_string(),
            revision: result.new_revision.get(),
            references,
            orphan_sources,
            _next_hint,
        }))
    }

    // ---------------------------------------------------------------------
    // Tool 14: configure page chrome
    // ---------------------------------------------------------------------
    #[tool(
        name = "configure_page_chrome",
        description = "Configure A4 paged header and footer slots (left/center/right) with template placeholders ({page}, {total}, {title}, {subtitle}, {date}, {kicker}, {authors}) and first-page visibility."
    )]
    pub async fn configure_page_chrome(
        &self,
        Extension(parts): Extension<Parts>,
        Parameters(request): Parameters<ConfigurePageChromeParams>,
    ) -> Result<Json<ConfigurePageChromeReceipt>, ErrorData> {
        let actor = request_context(&parts)?.actor;
        let doc_id = parse_doc_id(&request.doc_id)?;
        if let Some(expected) = request.base_revision {
            let current = self
                .state
                .store
                .get_document(&doc_id)
                .await
                .map_err(mcp_error)?;
            let current_revision = current.revision.get();
            if u64::try_from(current_revision).ok() != Some(expected) {
                return Err(invalid_params(
                    format!(
                        "base_revision {expected} does not match current revision {current_revision}"
                    ),
                    "Reload the document and retry with the current revision.",
                    Some(json!({"current_revision": current_revision})),
                ));
            }
        }
        let (result, updated) = self
            .apply_with_reload(&doc_id, &actor, "configure_page_chrome", |document| {
                let mut theme = document.theme.clone();
                theme.page_chrome = Some(merge_page_chrome(theme.page_chrome.as_ref(), &request));
                Ok(vec![PatchOp::SetTheme { theme }])
            })
            .await?;
        let page_chrome = updated.theme.page_chrome.clone().unwrap_or_default();
        let _next_hint = format!(
            "Page chrome committed at revision {}. Slots support {{page}}, {{total}}, {{title}}, {{subtitle}}, {{date}}, {{kicker}} and {{authors}}.",
            result.new_revision
        );
        Ok(Json(ConfigurePageChromeReceipt {
            doc_id: doc_id.to_string(),
            revision: result.new_revision.get(),
            page_chrome,
            _next_hint,
        }))
    }

    // ---------------------------------------------------------------------
    // Tool 15: update style and CSS
    // ---------------------------------------------------------------------
    #[tool(
        name = "update_style_and_css",
        description = "Update theme preset, CSS custom-property tokens and scoped custom_css. Validates token names/values and CSS braces/strings/comments with exact line/column errors."
    )]
    pub async fn update_style_and_css(
        &self,
        Extension(parts): Extension<Parts>,
        Parameters(request): Parameters<UpdateStyleParams>,
    ) -> Result<Json<UpdateStyleReceipt>, ErrorData> {
        let actor = request_context(&parts)?.actor;
        for (name, value) in &request.tokens_patch {
            if !is_valid_css_token_name(name) {
                return Err(invalid_params(
                    format!("invalid CSS token name `{name}`"),
                    "Token names must start with `--` and contain only ASCII letters, digits, '-' or '_'.",
                    Some(
                        json!({"valid_examples":["--rpt-primary","--rpt-accent","--rpt-font-body"]}),
                    ),
                ));
            }
            if !is_safe_css_token_value(value) {
                return Err(invalid_params(
                    format!("unsafe CSS token value for `{name}`"),
                    "Avoid `;`, `{`, `}`, `<`, `>` and backslashes in token values.",
                    None,
                ));
            }
        }
        if let Some(css) = &request.custom_css
            && let Err(error) = quire_protocol::validate_custom_css(css)
        {
            return Err(protocol_error(error));
        }
        let doc_id = parse_doc_id(&request.doc_id)?;
        let (result, updated) = self
            .apply_with_reload(&doc_id, &actor, "update_style_and_css", |document| {
                let mut theme = document.theme.clone();
                if let Some(preset) = &request.preset {
                    theme.preset = preset.clone();
                }
                for (name, value) in &request.tokens_patch {
                    theme.tokens.insert(name.clone(), value.clone());
                }
                if let Some(custom_css) = &request.custom_css {
                    theme.custom_css = match request.css_mode {
                        CssMode::Append if !theme.custom_css.is_empty() => {
                            format!("{}\n{}", theme.custom_css.trim_end(), custom_css)
                        }
                        _ => custom_css.clone(),
                    };
                }
                Ok(vec![PatchOp::SetTheme {
                    theme: theme.clone(),
                }])
            })
            .await?;
        let theme = updated.theme;
        let _next_hint = format!(
            "Theme committed at revision {}. Run lint_document to re-check CSS and source usage.",
            result.new_revision
        );
        Ok(Json(UpdateStyleReceipt {
            doc_id: doc_id.to_string(),
            revision: result.new_revision.get(),
            theme: theme.summary(),
            token_count: theme.tokens.len(),
            has_custom_css: !theme.custom_css.is_empty(),
            _next_hint,
        }))
    }

    // ---------------------------------------------------------------------
    // Tool 15: lint
    // ---------------------------------------------------------------------
    #[tool(
        name = "lint_document",
        description = "Run zero-browser static validation: dangling/unused citations, table schema/row keys, CSS syntax, custom_html fallback, heading/TOC/chart rules."
    )]
    pub async fn lint_document(
        &self,
        Parameters(request): Parameters<LintParams>,
    ) -> Result<Json<LintReceipt>, ErrorData> {
        let doc_id = parse_doc_id(&request.doc_id)?;
        let document = self
            .state
            .store
            .get_document(&doc_id)
            .await
            .map_err(mcp_error)?;
        let report = lint_document(&document);
        let rev = document.revision.get();
        if report.is_clean() {
            return Ok(Json(LintReceipt {
                ok: true,
                rev,
                summary: None,
                diagnostics: Vec::new(),
                _next_hint: None,
            }));
        }
        let ok = !report.has_errors();
        let _next_hint = if report.has_errors() {
            format!(
                "Fix {} error(s) using each diagnostic's path and hint.",
                report.summary.errors
            )
        } else {
            format!(
                "0 errors, {} warning(s); review warnings or call read_as_markdown.",
                report.summary.warnings
            )
        };
        Ok(Json(LintReceipt {
            ok,
            rev,
            summary: Some(report.summary),
            diagnostics: report.diagnostics,
            _next_hint: Some(_next_hint),
        }))
    }

    // ---------------------------------------------------------------------
    // Tool 16: list comments
    // ---------------------------------------------------------------------
    #[tool(
        name = "list_comments",
        description = "List side-channel review threads. Archived threads are omitted unless status=archived is supplied. Includes anchor_snapshot and live anchor_detached flags."
    )]
    pub async fn list_comments(
        &self,
        Parameters(request): Parameters<ListCommentsParams>,
    ) -> Result<Json<ListCommentsReceipt>, ErrorData> {
        let doc_id = parse_doc_id(&request.doc_id)?;
        let block_id = request
            .block_id
            .as_deref()
            .map(parse_block_id)
            .transpose()?;
        let mut comments = self
            .state
            .store
            .list_comment_threads(&doc_id, request.status, block_id.as_ref())
            .await
            .map_err(mcp_error)?;
        if request.status.is_none() {
            comments.retain(|thread| thread.status != CommentStatus::Archived);
        }
        let count = comments.len();
        let _next_hint = if comments.is_empty() {
            "No matching review comments. Human reviewers can create threads with POST /api/docs/{doc_id}/comments.".to_string()
        } else {
            "Reply with reply_comment(doc_id, thread_id, body, mark_addressed=true, addressed_rev=...) after fixing the block. Only humans can archive a thread.".to_string()
        };
        Ok(Json(ListCommentsReceipt {
            doc_id: doc_id.to_string(),
            comments,
            count,
            _next_hint,
        }))
    }

    // ---------------------------------------------------------------------
    // Tool 17: add comment
    // ---------------------------------------------------------------------
    #[tool(
        name = "add_comment",
        description = "Create an agent review comment anchored to a block. Comments are side-channel data: they never increment current_rev or participate in Smart CAS."
    )]
    pub async fn add_comment(
        &self,
        Extension(parts): Extension<Parts>,
        Parameters(request): Parameters<AddCommentParams>,
    ) -> Result<Json<AddCommentReceipt>, ErrorData> {
        let actor = request_context(&parts)?.actor;
        let doc_id = parse_doc_id(&request.doc_id)?;
        let block_id = parse_block_id(&request.block_id)?;
        if let Some(quote) = &request.quote {
            quote.validate().map_err(|reason| {
                invalid_params(
                    reason,
                    "Quote the exact selected phrase; prefix/suffix provide context.",
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
                    author_type: CommentAuthorType::Agent,
                    author: actor,
                    body: request.body,
                    quote: request.quote,
                },
            )
            .await
            .map_err(mcp_error)?;
        self.state.publish_comment_updated(&thread).await;
        let _next_hint = format!(
            "Comment thread `{}` created without a revision change. Writers should reply with reply_comment after editing.",
            thread.thread_id
        );
        Ok(Json(AddCommentReceipt { thread, _next_hint }))
    }

    // ---------------------------------------------------------------------
    // Tool 18: reply comment
    // ---------------------------------------------------------------------
    #[tool(
        name = "reply_comment",
        description = "Reply to a review thread. With mark_addressed=true an open thread moves to addressed and records addressed_rev; agents cannot archive or reopen threads."
    )]
    pub async fn reply_comment(
        &self,
        Extension(parts): Extension<Parts>,
        Parameters(request): Parameters<ReplyCommentParams>,
    ) -> Result<Json<ReplyCommentReceipt>, ErrorData> {
        let actor = request_context(&parts)?.actor;
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
        Ok(Json(ReplyCommentReceipt { thread, _next_hint }))
    }
}

// -------------------------------------------------------------------------
// Internal helpers
// -------------------------------------------------------------------------

impl CoverPatch {
    /// Validate and apply a partial cover update, returning self-healing errors on bad input.
    pub(crate) fn apply_to(&self, cover: &mut CoverConfig) -> Result<(), ErrorData> {
        if let Some(enabled) = self.enabled {
            cover.enabled = enabled;
        }
        if let Some(variant) = &self.variant {
            if !COVER_VARIANTS.contains(&variant.as_str()) {
                return Err(invalid_params(
                    format!("unknown cover variant `{variant}`"),
                    format!("Valid cover variants: {}", COVER_VARIANTS.join(", ")),
                    Some(json!({"valid_variants": COVER_VARIANTS})),
                ));
            }
            cover.variant = variant.clone();
        }
        if let Some(title) = &self.title {
            if title.trim().is_empty() {
                return Err(invalid_params(
                    "cover title must not be empty",
                    "Supply a title or omit the field.",
                    None,
                ));
            }
            cover.title = title.clone();
        }
        if let Some(subtitle) = &self.subtitle {
            cover.subtitle = (!subtitle.trim().is_empty()).then(|| subtitle.clone());
        }
        if let Some(authors) = &self.authors {
            cover.authors = authors.clone();
        }
        if let Some(date) = &self.date {
            cover.date = (!date.trim().is_empty()).then(|| date.clone());
        }
        if let Some(meta) = &self.meta {
            cover.meta = meta.clone();
        }
        if let Some(custom_html) = &self.custom_html {
            cover.custom_html = (!custom_html.trim().is_empty()).then(|| custom_html.clone());
        }
        Ok(())
    }
}

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
pub(crate) fn build_digest(document: &ReportDocument, query: Option<&str>) -> DocumentDigest {
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
        .map(block_digest)
        .collect::<Vec<_>>();
    DocumentDigest {
        title: document.title(),
        current_rev: document.revision.get(),
        theme: document.theme.summary(),
        toc: document.toc.clone(),
        block_count: document.blocks.len(),
        source_count: document.sources.len(),
        blocks,
    }
}

impl QuireMcpServer {
    async fn read_draft(&self, path: &str) -> Result<ReportDraft, ErrorData> {
        let bytes = self.read_workspace_file(path).await?;
        let text = String::from_utf8(bytes).map_err(|error| {
            invalid_params(
                format!("draft file is not valid UTF-8: {error}"),
                "Save draft.json as UTF-8.",
                None,
            )
        })?;
        ReportDraft::parse(&text).map_err(|error| {
            invalid_params(
                error.to_string(),
                "Draft must be canonical ReportDocument/ReportDraft JSON with a blocks array.",
                None,
            )
        })
    }

    async fn read_workspace_file(&self, path: &str) -> Result<Vec<u8>, ErrorData> {
        let config: &Config = &self.state.config;
        let raw = Path::new(path);
        let candidate = if raw.is_absolute() {
            raw.to_path_buf()
        } else {
            config.workspace_dir.join(raw)
        };
        let workspace = config.workspace_dir.canonicalize().map_err(|error| {
            invalid_params(
                format!("workspace directory is unavailable: {error}"),
                "Set QUIRE_WORKSPACE_DIR to an existing directory.",
                None,
            )
        })?;
        let canonical = candidate.canonicalize().map_err(|error| {
            invalid_params(
                format!("draft file `{path}` not found: {error}"),
                "Write the draft under QUIRE_WORKSPACE_DIR and pass a relative path.",
                None,
            )
        })?;
        if !canonical.starts_with(&workspace) {
            return Err(invalid_params(
                format!("draft path `{path}` escapes the workspace"),
                "Only files under QUIRE_WORKSPACE_DIR may be read.",
                None,
            ));
        }
        let metadata = tokio::fs::metadata(&canonical)
            .await
            .map_err(|error| invalid_params(error.to_string(), "Check the draft path.", None))?;
        if metadata.len() as usize > config.max_draft_bytes {
            return Err(invalid_params(
                format!(
                    "draft file is {} bytes; limit is {}",
                    metadata.len(),
                    config.max_draft_bytes
                ),
                "Split the draft into smaller chapter files.",
                None,
            ));
        }
        tokio::fs::read(&canonical)
            .await
            .map_err(|error| invalid_params(error.to_string(), "Check file permissions.", None))
    }

    async fn apply_with_reload<F>(
        &self,
        doc_id: &DocId,
        actor: &str,
        summary: &str,
        mut build_ops: F,
    ) -> Result<(ApplyResult, ReportDocument), ErrorData>
    where
        F: FnMut(&ReportDocument) -> Result<Vec<PatchOp>, ErrorData>,
    {
        for attempt in 0..4 {
            let document = self
                .state
                .store
                .get_document(doc_id)
                .await
                .map_err(mcp_error)?;
            let ops = build_ops(&document)?;
            match self
                .state
                .store
                .apply_operations(doc_id, document.revision, actor, summary, &ops)
                .await
            {
                Ok(result) => {
                    let updated = self
                        .state
                        .store
                        .get_document(doc_id)
                        .await
                        .map_err(mcp_error)?;
                    return Ok((result, updated));
                }
                Err(StoreError::RevisionAhead { .. } | StoreError::Conflict(_)) if attempt < 3 => {
                    tokio::task::yield_now().await;
                    continue;
                }
                Err(error) => return Err(mcp_error(error)),
            }
        }
        Err(invalid_params(
            "configuration update could not be rebased after 4 attempts",
            "Reload the document and retry set_cover/configure_*.",
            None,
        ))
    }
}

fn is_retryable_conflict(error: &ErrorData) -> bool {
    error
        .data
        .as_ref()
        .and_then(|data| data.get("code"))
        .and_then(Value::as_str)
        .is_some_and(|code| matches!(code, "REVISION_CONFLICT" | "REVISION_AHEAD"))
}

fn touched_blocks(ops: &[PatchOp], assigned: &[IdAssignment]) -> BTreeSet<BlockId> {
    let mut touched: BTreeSet<BlockId> = ops.iter().flat_map(PatchOp::touched_block_ids).collect();
    touched.extend(
        assigned
            .iter()
            .map(|assignment| assignment.assigned.clone()),
    );
    touched
}

fn touched_draft_blocks(blocks: &[Block], assigned: &[IdAssignment]) -> BTreeSet<BlockId> {
    let mut touched: BTreeSet<BlockId> = blocks.iter().map(|block| block.id.clone()).collect();
    touched.extend(
        assigned
            .iter()
            .map(|assignment| assignment.assigned.clone()),
    );
    touched
}

fn split_scoped_diagnostics(
    diagnostics: Vec<Diagnostic>,
    touched: &BTreeSet<BlockId>,
) -> (Vec<Diagnostic>, Vec<Diagnostic>) {
    let scoped: Vec<Diagnostic> = if touched.is_empty() {
        diagnostics
    } else {
        diagnostics
            .into_iter()
            .filter(|diagnostic| {
                diagnostic
                    .block_id
                    .as_ref()
                    .is_some_and(|block_id| touched.contains(block_id))
            })
            .collect()
    };
    scoped
        .into_iter()
        .partition(|diagnostic| diagnostic.severity == Severity::Error)
}

fn draft_receipt(
    result: ApplyResult,
    blocks: &[Block],
    base_hint: String,
) -> Json<ApplyDraftReceipt> {
    let touched = touched_draft_blocks(blocks, &result.outcome.assigned_ids);
    let (blocking_errors, inline_warnings) =
        split_scoped_diagnostics(result.inline_warnings, &touched);
    let _next_hint = if !blocking_errors.is_empty() {
        format!(
            "Draft committed as revision {} with {} blocking error(s); fix `blocking_errors`, then lint again.",
            result.new_revision,
            blocking_errors.len()
        )
    } else if result.outcome.no_op {
        format!(
            "Draft was a no-op; no revision was created. Current revision is {}.",
            result.new_revision
        )
    } else if result.rebased {
        format!(
            "Draft committed as revision {} after rebasing over {} revision(s) from base {}. {base_hint}",
            result.new_revision, result.rebased_over, result.base_revision
        )
    } else {
        base_hint
    };
    Json(ApplyDraftReceipt {
        new_revision: result.new_revision.get(),
        base_revision: result.base_revision.get(),
        rebased: result.rebased,
        rebased_over: result.rebased_over,
        committed: true,
        assigned_ids: result.outcome.assigned_ids,
        inline_warnings,
        blocking_errors,
        _next_hint,
    })
}

/// Map a structured protocol failure into an MCP error that exposes stable recovery fields.
fn protocol_error(error: ProtocolError) -> ErrorData {
    let code = error.code();
    let hint = error.hint();
    let message = error.to_string();
    ErrorData::invalid_params(
        message.clone(),
        Some(json!({
            "code": code,
            "message": message,
            "hint": hint,
            "protocol_error": error,
        })),
    )
}

fn request_context(parts: &Parts) -> Result<RequestContext, ErrorData> {
    RequestContext::from_parts(parts).map_err(|error| {
        invalid_params(
            error.to_string(),
            "Use 1-64 letters/digits plus '-' '_' '.' in X-Session-Id and a short X-Actor.",
            None,
        )
    })
}

fn parse_doc_id(raw: &str) -> Result<DocId, ErrorData> {
    DocId::parse(raw).map_err(protocol_error)
}

fn parse_block_id(raw: &str) -> Result<BlockId, ErrorData> {
    BlockId::parse(raw).map_err(protocol_error)
}

fn parse_block_ids(raw: &[String]) -> Result<Vec<BlockId>, ErrorData> {
    raw.iter().map(|value| parse_block_id(value)).collect()
}

fn parse_revision(value: i64) -> Result<Revision, ErrorData> {
    Revision::new(value).map_err(protocol_error)
}

fn generate_doc_id(title: &str) -> Result<DocId, ErrorData> {
    let slug = slugify_title(title);
    let suffix = short_token(8);
    let prefix: String = slug.chars().take(48).collect();
    DocId::parse(format!("{prefix}-{suffix}")).map_err(protocol_error)
}

fn slugify_title(input: &str) -> String {
    let mut output = String::new();
    let mut pending_dash = false;
    for ch in input.chars() {
        if ch.is_ascii_alphanumeric() {
            if pending_dash && !output.is_empty() {
                output.push('-');
            }
            pending_dash = false;
            for lower in ch.to_lowercase() {
                output.push(lower);
            }
        } else {
            pending_dash = true;
        }
        if output.len() >= 80 {
            break;
        }
    }
    let trimmed = output.trim_matches('-').to_string();
    if trimmed.is_empty() {
        "report".to_string()
    } else {
        trimmed
    }
}

fn draft_to_document(
    mut draft: ReportDraft,
    fallback_doc_id: &str,
) -> Result<ReportDocument, ErrorData> {
    if draft.cover.title.trim().is_empty()
        && let Some(title) = draft.title.take()
    {
        draft.cover.title = title;
    }
    if draft.cover.title.trim().is_empty() {
        draft.cover.title = "Untitled Report".to_string();
    }
    let doc_id = match draft.doc_id {
        Some(doc_id) => doc_id,
        None => parse_doc_id(fallback_doc_id)?,
    };
    Ok(ReportDocument {
        doc_id,
        session_id: None,
        revision: Revision::ZERO,
        cover: draft.cover,
        toc: draft.toc,
        theme: draft.theme,
        sources: draft.sources,
        blocks: draft.blocks,
        references: draft.references,
    })
}

fn merge_sources(
    existing: &IndexMap<SourceId, SourceEntry>,
    incoming: &IndexMap<SourceId, SourceEntry>,
) -> (
    IndexMap<SourceId, SourceEntry>,
    BTreeMap<SourceId, SourceId>,
) {
    let mut merged = existing.clone();
    let mut key_to_id: BTreeMap<String, SourceId> = merged
        .iter()
        .map(|(id, source)| (normalized_source_key(source), id.clone()))
        .collect();
    let mut canonical_map = BTreeMap::new();
    for (source_id, source) in incoming {
        let key = normalized_source_key(source);
        let canonical_id = if let Some(existing_id) = key_to_id.get(&key) {
            existing_id.clone()
        } else {
            source_id.clone()
        };
        if canonical_id != *source_id {
            if let Some(existing_source) = merged.get(&canonical_id).cloned() {
                let combined = merge_source_entries(&existing_source, source);
                key_to_id.insert(normalized_source_key(&combined), canonical_id.clone());
                merged.insert(canonical_id.clone(), combined);
            }
            canonical_map.insert(source_id.clone(), canonical_id);
            continue;
        }
        let combined = merged
            .get(source_id)
            .map(|existing_source| merge_source_entries(existing_source, source))
            .unwrap_or_else(|| source.clone());
        key_to_id.insert(normalized_source_key(&combined), source_id.clone());
        merged.insert(source_id.clone(), combined);
        canonical_map.insert(source_id.clone(), source_id.clone());
    }
    (merged, canonical_map)
}

fn rewrite_source_references(block: &mut Block, mapping: &BTreeMap<SourceId, SourceId>) {
    if mapping.is_empty() {
        return;
    }
    let mut seen = HashSet::new();
    block.source_ids = block
        .source_ids
        .iter()
        .map(|source_id| {
            mapping
                .get(source_id)
                .cloned()
                .unwrap_or_else(|| source_id.clone())
        })
        .filter(|source_id| seen.insert(source_id.clone()))
        .collect();
    let rewrite = |text: &mut String| {
        for (old, new) in mapping {
            if old != new {
                *text = text.replace(&format!("[^{old}]"), &format!("[^{new}]"));
            }
        }
    };
    match &mut block.body {
        BlockKind::Heading { text, .. }
        | BlockKind::Paragraph { text }
        | BlockKind::Quote { text } => rewrite(text),
        BlockKind::Table(table) => {
            if let Some(title) = &mut table.title {
                rewrite(title);
            }
        }
        BlockKind::Echarts(_) | BlockKind::CustomHtml(_) => {}
    }
}

/// Reassign only empty/colliding IDs and report exactly those changes.
fn unique_block_ids(existing_ids: &HashSet<BlockId>, blocks: &mut [Block]) -> Vec<IdAssignment> {
    let mut used: HashSet<BlockId> = existing_ids.clone();
    let mut assignments = Vec::new();
    for block in blocks {
        let original = block.id.clone();
        let pending = original.is_pending();
        if pending || used.contains(&original) {
            let prefix = block.block_type();
            let mut sequence = 1usize;
            let mut assigned = original.clone();
            loop {
                let candidate = format!("b_{prefix}_{sequence}");
                if candidate.len() <= 64
                    && let Ok(parsed) = BlockId::parse(candidate)
                    && !used.contains(&parsed)
                {
                    assigned = parsed;
                    break;
                }
                sequence += 1;
                if sequence > 100_000 {
                    break;
                }
            }
            block.id = assigned.clone();
            used.insert(assigned.clone());
            assignments.push(IdAssignment {
                requested: (!pending).then_some(original),
                assigned,
            });
        } else {
            used.insert(original.clone());
        }
    }
    assignments
}

fn inline_source_upserts(
    sources: Option<IndexMap<SourceId, SourceInput>>,
) -> Result<Vec<SourceUpsert>, ErrorData> {
    let Some(sources) = sources else {
        return Ok(Vec::new());
    };
    Ok(sources
        .into_iter()
        .map(|(source_id, input)| SourceUpsert {
            source_id,
            source: input.entry,
        })
        .collect())
}

/// Trim, validate and de-duplicate preview IDs while preserving producer order.
fn normalize_preview_ids(raw: Vec<String>) -> Result<Vec<String>, ErrorData> {
    let mut seen = HashSet::new();
    let mut preview_ids = Vec::with_capacity(raw.len());
    for preview_id in raw {
        let preview_id = preview_id.trim();
        if preview_id.is_empty() {
            return Err(invalid_params(
                "preview_ids must not contain empty values",
                "Pass the Runtime tool_call_id of each ephemeral preview to clear.",
                None,
            ));
        }
        if seen.insert(preview_id.to_string()) {
            preview_ids.push(preview_id.to_string());
        }
    }
    Ok(preview_ids)
}

/// Publish an immediate cleanup frame when a no-op commit leaves previews behind.
async fn publish_ephemeral_clear(state: &AppState, doc_id: &DocId, preview_ids: &[String]) {
    if preview_ids.is_empty() {
        return;
    }
    let payload = EphemeralPayload::Clear {
        event: EphemeralClearEvent::new(doc_id.clone(), preview_ids.to_vec()),
    };
    if let Err(error) = state
        .store
        .bus()
        .publish_ephemeral(doc_id.as_str(), &payload)
        .await
    {
        tracing::warn!(%error, %doc_id, "failed to publish ephemeral clear");
    }
}

fn draft_source_upserts(
    draft: &ReportDraft,
    inline: Option<IndexMap<SourceId, SourceInput>>,
) -> Result<Vec<SourceUpsert>, ErrorData> {
    let mut combined: IndexMap<SourceId, SourceEntry> = draft.sources.clone();
    if let Some(inline) = inline {
        for (source_id, input) in inline {
            combined.insert(source_id, input.entry);
        }
    }
    Ok(combined
        .into_iter()
        .map(|(source_id, source)| SourceUpsert { source_id, source })
        .collect())
}

// The apply_with_reload helper is defined separately below to keep tool signatures tidy.
impl QuireMcpServer {
    async fn create_from_draft(
        &self,
        draft: ReportDraft,
        inline_sources: Option<IndexMap<SourceId, SourceInput>>,
        parts: &Parts,
        actor: &str,
        preview_ids: &[String],
    ) -> Result<Json<ApplyDraftReceipt>, ErrorData> {
        let source_upserts = draft_source_upserts(&draft, inline_sources)?;
        let combined_sources: IndexMap<SourceId, SourceEntry> = source_upserts
            .into_iter()
            .map(|upsert| (upsert.source_id, upsert.source))
            .collect();
        let (sources, canonical) = merge_sources(&IndexMap::new(), &combined_sources);
        let mut blocks = draft.blocks.clone();
        for block in &mut blocks {
            rewrite_source_references(block, &canonical);
        }
        let existing_ids = HashSet::new();
        let assigned_ids = unique_block_ids(&existing_ids, &mut blocks);
        let title = if !draft.cover.title.trim().is_empty() {
            draft.cover.title.clone()
        } else {
            draft
                .title
                .clone()
                .unwrap_or_else(|| "Untitled Report".into())
        };
        let doc_id = match draft.doc_id {
            Some(doc_id) => doc_id,
            None => generate_doc_id(&title)?,
        };
        let session_id = RequestContext::from_parts(parts)
            .map_err(|error| {
                invalid_params(error.to_string(), "Use a valid X-Session-Id header.", None)
            })?
            .session_id;
        let mut document = ReportDocument {
            doc_id: doc_id.clone(),
            session_id,
            revision: Revision::ZERO,
            cover: CoverConfig {
                title: title.clone(),
                ..draft.cover
            },
            toc: draft.toc,
            theme: draft.theme,
            sources,
            blocks: blocks.clone(),
            references: draft.references,
        };
        document.cover.title = title;
        document.cover.enabled = true;
        let normalization_warnings = quire_protocol::normalize::normalize_document(&mut document);
        let result = self
            .state
            .store
            .create_document_with_cleared_previews(&document, actor, None, preview_ids.to_vec())
            .await
            .map_err(mcp_error)?;
        let touched = touched_draft_blocks(&blocks, &assigned_ids);
        let (blocking_errors, inline_warnings) =
            split_scoped_diagnostics(normalization_warnings, &touched);
        let revision = result.revision.get();
        let _next_hint = if blocking_errors.is_empty() {
            format!(
                "Draft created report `{}` at revision {}. Run lint_document, then continue at expected_revision={}.",
                result.doc_id, result.revision, result.revision
            )
        } else {
            format!(
                "Draft created report `{}` at revision {} with {} blocking error(s); fix `blocking_errors` before continuing.",
                result.doc_id,
                result.revision,
                blocking_errors.len()
            )
        };
        Ok(Json(ApplyDraftReceipt {
            new_revision: revision,
            base_revision: 0,
            rebased: false,
            rebased_over: 0,
            committed: true,
            assigned_ids,
            inline_warnings,
            blocking_errors,
            _next_hint,
        }))
    }

    async fn replace_with_draft(
        &self,
        target_doc_id: &DocId,
        draft: ReportDraft,
        inline_sources: Option<IndexMap<SourceId, SourceInput>>,
        actor: &str,
        expected_revision: Option<Revision>,
        preview_ids: &[String],
    ) -> Result<Json<ApplyDraftReceipt>, ErrorData> {
        let existing = self
            .state
            .store
            .get_document(target_doc_id)
            .await
            .map_err(mcp_error)?;
        let source_upserts = draft_source_upserts(&draft, inline_sources)?;
        let blocks = draft.blocks.clone();
        let replace_op = if existing.blocks.is_empty() {
            PatchOp::Append {
                blocks: blocks.clone(),
            }
        } else {
            PatchOp::ReplaceRange {
                start_id: existing
                    .blocks
                    .first()
                    .map(|block| block.id.clone())
                    .ok_or_else(|| {
                        invalid_params(
                            "document has no blocks to replace",
                            "Append blocks instead.",
                            None,
                        )
                    })?,
                end_id: existing
                    .blocks
                    .last()
                    .map(|block| block.id.clone())
                    .ok_or_else(|| {
                        invalid_params(
                            "document has no blocks to replace",
                            "Append blocks instead.",
                            None,
                        )
                    })?,
                blocks: blocks.clone(),
            }
        };
        let mut cover = draft.cover.clone();
        if cover.title.trim().is_empty() {
            cover.title = existing.cover.title.clone();
        }
        let ops = vec![
            PatchOp::SetCover { cover },
            PatchOp::SetToc { toc: draft.toc },
            PatchOp::SetTheme { theme: draft.theme },
            PatchOp::SetReferences {
                references: draft.references,
            },
            replace_op,
        ];
        let result = self
            .state
            .store
            .apply_operations_with_extras(
                target_doc_id,
                expected_revision.unwrap_or(existing.revision),
                actor,
                "replace report from draft",
                &ops,
                CommitExtras {
                    sources: source_upserts,
                    cleared_preview_ids: preview_ids.to_vec(),
                },
            )
            .await
            .map_err(mcp_error)?;
        if result.outcome.no_op {
            publish_ephemeral_clear(&self.state, target_doc_id, preview_ids).await;
        }
        Ok(draft_receipt(
            result,
            &blocks,
            "Run lint_document and read_as_markdown.".to_string(),
        ))
    }

    async fn append_draft(
        &self,
        target_doc_id: &DocId,
        draft: ReportDraft,
        inline_sources: Option<IndexMap<SourceId, SourceInput>>,
        actor: &str,
        expected_revision: Option<Revision>,
        preview_ids: &[String],
    ) -> Result<Json<ApplyDraftReceipt>, ErrorData> {
        let existing = self
            .state
            .store
            .get_document(target_doc_id)
            .await
            .map_err(mcp_error)?;
        let source_upserts = draft_source_upserts(&draft, inline_sources)?;
        let blocks = draft.blocks.clone();
        let ops = vec![PatchOp::Append {
            blocks: blocks.clone(),
        }];
        let result = self
            .state
            .store
            .apply_operations_with_extras(
                target_doc_id,
                expected_revision.unwrap_or(existing.revision),
                actor,
                "append draft section",
                &ops,
                CommitExtras {
                    sources: source_upserts,
                    cleared_preview_ids: preview_ids.to_vec(),
                },
            )
            .await
            .map_err(mcp_error)?;
        if result.outcome.no_op {
            publish_ephemeral_clear(&self.state, target_doc_id, preview_ids).await;
        }
        Ok(draft_receipt(
            result,
            &blocks,
            "Call lint_document and read_as_markdown.".to_string(),
        ))
    }

    // Range drafts genuinely need all coordinates; bundling them into a one-use struct would
    // make the MCP handler harder to follow.
    #[allow(clippy::too_many_arguments)]
    async fn replace_range_with_draft(
        &self,
        target_doc_id: &DocId,
        draft: ReportDraft,
        start_id: &BlockId,
        end_id: &BlockId,
        inline_sources: Option<IndexMap<SourceId, SourceInput>>,
        actor: &str,
        expected_revision: Option<Revision>,
        preview_ids: &[String],
    ) -> Result<Json<ApplyDraftReceipt>, ErrorData> {
        let existing = self
            .state
            .store
            .get_document(target_doc_id)
            .await
            .map_err(mcp_error)?;
        let source_upserts = draft_source_upserts(&draft, inline_sources)?;
        let blocks = draft.blocks.clone();
        let ops = vec![PatchOp::ReplaceRange {
            start_id: start_id.clone(),
            end_id: end_id.clone(),
            blocks: blocks.clone(),
        }];
        let result = self
            .state
            .store
            .apply_operations_with_extras(
                target_doc_id,
                expected_revision.unwrap_or(existing.revision),
                actor,
                "replace range from draft",
                &ops,
                CommitExtras {
                    sources: source_upserts,
                    cleared_preview_ids: preview_ids.to_vec(),
                },
            )
            .await
            .map_err(mcp_error)?;
        if result.outcome.no_op {
            publish_ephemeral_clear(&self.state, target_doc_id, preview_ids).await;
        }
        Ok(draft_receipt(
            result,
            &blocks,
            format!("Draft replaced range {start_id}..{end_id}. Run lint_document."),
        ))
    }
}

fn merge_page_chrome(
    current: Option<&PageChromeConfig>,
    request: &ConfigurePageChromeParams,
) -> PageChromeConfig {
    let mut chrome = current.cloned().unwrap_or_default();
    if let Some(header) = request.header.clone() {
        chrome.header = header;
    }
    if let Some(footer) = request.footer.clone() {
        chrome.footer = footer;
    }
    if let Some(hide) = request.hide_header_on_first_page {
        chrome.hide_header_on_first_page = hide;
    }
    if let Some(hide) = request.hide_footer_on_first_page {
        chrome.hide_footer_on_first_page = hide;
    }
    chrome
}

#[cfg(test)]
mod tests {
    use super::*;
    use quire_protocol::PageChromeSlots;

    #[test]
    fn slugify_title_drops_non_ascii_characters() {
        assert_eq!(slugify_title("Q3 Revenue 报告"), "q3-revenue");
        assert_eq!(slugify_title("你好"), "report");
        assert_eq!(slugify_title("Report_2026"), "report-2026");
        assert_eq!(slugify_title("!!!"), "report");
    }

    #[test]
    fn generated_doc_ids_start_with_ascii_letters() {
        let generated = generate_doc_id("你好");
        assert!(generated.is_ok());
        if let Ok(doc_id) = generated {
            assert!(doc_id.as_str().starts_with("report-"));
        }
    }

    #[test]
    fn page_chrome_merges_only_supplied_fields() {
        let current = PageChromeConfig {
            header: PageChromeSlots {
                left: "{kicker}".into(),
                center: String::new(),
                right: "{title}".into(),
            },
            footer: PageChromeSlots {
                left: "{title}".into(),
                center: String::new(),
                right: "第 {page} 页".into(),
            },
            hide_header_on_first_page: true,
            hide_footer_on_first_page: false,
        };
        let request = ConfigurePageChromeParams {
            doc_id: "doc".into(),
            header: None,
            footer: Some(PageChromeSlots {
                left: String::new(),
                center: "— {page} / {total} —".into(),
                right: String::new(),
            }),
            hide_header_on_first_page: Some(false),
            hide_footer_on_first_page: None,
            base_revision: None,
        };
        let merged = merge_page_chrome(Some(&current), &request);
        assert_eq!(merged.header.left, "{kicker}");
        assert_eq!(merged.footer.center, "— {page} / {total} —");
        assert!(!merged.hide_header_on_first_page);
        assert!(!merged.hide_footer_on_first_page);
    }
}
