use super::*;

#[tool_router(router = kb_router, vis = "pub(crate)")]
impl AdminMcpServer {
    // ---------------------------------------------------------------------
    // Branch and release tools
    // ---------------------------------------------------------------------

    /// Render the report in a headless browser via Playwright MCP and return an MCP `ImageContent` screenshot plus structured DOM overflow and CSS diagnostics.
    ///
    /// Scoping & mode guide:
    /// - Prefer scoping by `section_id`, `block_ids`, or `page` (`1`-based in `paged` mode) to keep screenshots focused.
    /// - When `section_id` or `block_ids` is set, cover, TOC, and bibliography chrome are automatically suppressed.
    /// - Set `include_image: false` when only `total_pages`, `layout_diagnostics.overflow_elements`, and computed `styles` are needed.
    #[tool(name = "preview_report")]
    pub async fn preview_report(
        &self,
        Parameters(request): Parameters<crate::preview::PreviewReportInput>,
    ) -> Result<CallToolResult, ErrorData> {
        let tools = crate::preview::PreviewToolService::new(
            std::sync::Arc::new(self.service.config().clone()),
            self.service.store().clone(),
            quire_server::report::QuireReportService::new(self.service.report_app_state()),
        );
        let result = tools
            .preview_report(quire_server::auth::RequestContext::system(), request)
            .await
            .map_err(admin_error)?;
        result.into_mcp_result()
    }

    /// List the active production knowledge-base branch, all `draft`/`active`/`archived` branches, and the 20 most recent immutable snapshots.
    ///
    /// Call this first before creating, diffing, promoting, or rolling back a knowledge-base branch so you have the current `active_branch` name and valid `recent_snapshots` version numbers.
    #[tool(name = "kb_list_branches")]
    pub async fn kb_list_branches(
        &self,
        Parameters(_request): Parameters<EmptyParams>,
    ) -> Result<Json<KbListBranchesReceipt>, ErrorData> {
        let branches = self
            .service
            .kb()
            .list_branches()
            .await
            .map_err(admin_error)?;
        let active_branch = self
            .service
            .kb()
            .active_branch()
            .await
            .map_err(admin_error)?
            .name;
        let recent_snapshots = self
            .service
            .kb()
            .list_snapshots(20)
            .await
            .map_err(admin_error)?;
        let next_hint = if branches
            .iter()
            .any(|branch| branch.status == KbBranchStatus::Draft)
        {
            "Call kb_diff_branch, edit the draft with kb_upsert_*, then kb_promote_branch."
                .to_string()
        } else {
            "Call kb_create_branch to start an isolated draft.".to_string()
        };
        Ok(Json(KbListBranchesReceipt {
            active_branch,
            branches,
            recent_snapshots,
            next_hint,
        }))
    }

    /// Clone an existing knowledge-base branch (defaulting to the current active branch) into a new isolated `draft` branch.
    ///
    /// Workflow:
    /// `kb_list_branches` -> `kb_create_branch` -> `kb_upsert_doc` / `kb_upsert_agent` / `kb_upsert_skill` -> test via `POST /api/sessions` with `kb_branch` -> `kb_diff_branch` -> `kb_promote_branch`.
    /// Never mutate the active production branch directly when iterating on prompts, agents, or skills.
    #[tool(name = "kb_create_branch")]
    pub async fn kb_create_branch(
        &self,
        Parameters(request): Parameters<KbCreateBranchParams>,
    ) -> Result<Json<KbCreateBranchReceipt>, ErrorData> {
        let branch = self
            .service
            .kb()
            .create_branch(&request.name, &request.description, request.from.as_deref())
            .await
            .map_err(admin_error)?;
        let next_hint = format!(
            "Edit branch `{}` with kb_upsert_* and inspect it with kb_diff_branch before promotion.",
            branch.name
        );
        Ok(Json(KbCreateBranchReceipt { branch, next_hint }))
    }

    /// Compute a three-way item-level diff (`added`, `modified`, `deleted`, and `conflict` flags) between a draft branch and the current active branch.
    ///
    /// Always run `kb_diff_branch` before calling `kb_promote_branch`. If any item reports `conflict: true`, reconcile that item on the draft branch with `kb_upsert_*` or `kb_delete_item` before promoting.
    #[tool(name = "kb_diff_branch")]
    pub async fn kb_diff_branch(
        &self,
        Parameters(request): Parameters<KbBranchParams>,
    ) -> Result<Json<KbDiffBranchReceipt>, ErrorData> {
        let diff = self
            .service
            .kb()
            .diff_branch(&request.name)
            .await
            .map_err(admin_error)?;
        let next_hint = if diff.changes.iter().any(|change| change.conflict) {
            "Resolve conflict items with kb_upsert_* or delete them, then run kb_diff_branch again."
                .to_string()
        } else {
            "Call kb_promote_branch to fast-forward or auto-merge into the active branch."
                .to_string()
        };
        Ok(Json(KbDiffBranchReceipt { diff, next_hint }))
    }

    /// Promote a `draft` branch into the active production knowledge base via three-way item merge, recording an immutable rollback snapshot and broadcasting cache invalidation.
    ///
    /// Important constraints:
    /// - The promoted draft branch transitions permanently to `archived` and cannot be edited again after promotion.
    /// - Overlapping edits on the same `(item_kind, item_key)` abort promotion with `KB_PROMOTE_CONFLICT` (`base`, `active`, and `branch` payloads) and advance the draft's `base_version` to the current active `head_version` so your next `kb_upsert_*` resolves the conflict cleanly.
    #[tool(name = "kb_promote_branch")]
    pub async fn kb_promote_branch(
        &self,
        Parameters(request): Parameters<KbBranchParams>,
    ) -> Result<Json<KbPromoteBranchReceipt>, ErrorData> {
        let report = self
            .service
            .kb()
            .promote_branch(&request.name)
            .await
            .map_err(admin_error)?;
        let next_hint = format!(
            "Active version {} is live and snapshot {} is immutable. Use kb_rollback_active to restore an earlier snapshot.",
            report.version, report.snapshot_version
        );
        Ok(Json(KbPromoteBranchReceipt { report, next_hint }))
    }

    /// Restore the active production knowledge base from an immutable snapshot version and publish a global KB cache invalidation.
    ///
    /// Before rollback, the current active state is automatically captured as a new immutable snapshot so the rollback itself can be reversed if needed.
    #[tool(name = "kb_rollback_active")]
    pub async fn kb_rollback_active(
        &self,
        Parameters(request): Parameters<KbRollbackParams>,
    ) -> Result<Json<KbRollbackReceipt>, ErrorData> {
        let report = self
            .service
            .kb()
            .rollback_active(request.version)
            .await
            .map_err(admin_error)?;
        let next_hint = format!(
            "Restored snapshot {} as active version {}. Cache invalidation was published.",
            report.restored_from_version, report.version
        );
        Ok(Json(KbRollbackReceipt { report, next_hint }))
    }

    // ---------------------------------------------------------------------
    // Branch item editing
    // ---------------------------------------------------------------------

    /// List knowledge-base items (`doc`, `agent`, `skill`) on a branch with their `item_key`, summary metadata, and `content_hash` without loading full bodies.
    ///
    /// Use this to discover valid `item_key` identifiers before calling `kb_read_item`, `kb_upsert_*`, or `kb_delete_item`.
    #[tool(name = "kb_list_items")]
    pub async fn kb_list_items(
        &self,
        Parameters(request): Parameters<KbListItemsParams>,
    ) -> Result<Json<KbListItemsReceipt>, ErrorData> {
        let mut items = self
            .service
            .kb()
            .list_items(&request.branch)
            .await
            .map_err(admin_error)?;
        if let Some(kind) = request.item_kind {
            items.retain(|item| item.item_kind == kind);
        }
        let next_hint = "Call kb_read_item with the item key for full content.".to_string();
        Ok(Json(KbListItemsReceipt {
            branch: request.branch,
            items,
            next_hint,
        }))
    }

    /// Read the complete payload of a single knowledge-base item (`doc`, `agent`, or `skill`, including all bundled skill `resources`) from a branch.
    ///
    /// Always call `kb_read_item` before updating an existing agent, skill, or document so you preserve fields (`allowed_tools`, `model`, or bundled skill `resources`) that `kb_upsert_*` replaces in full.
    #[tool(name = "kb_read_item")]
    pub async fn kb_read_item(
        &self,
        Parameters(request): Parameters<KbReadItemParams>,
    ) -> Result<Json<KbReadItemReceipt>, ErrorData> {
        let content = self
            .service
            .kb()
            .read_item(&request.branch, request.item_kind, &request.item_key)
            .await
            .map_err(admin_error)?;
        let found = content.is_some();
        let next_hint = if found {
            "Edit with kb_upsert_doc, kb_upsert_agent or kb_upsert_skill, then inspect kb_diff_branch."
                .to_string()
        } else {
            "Item not found. Call kb_list_items to discover valid keys.".to_string()
        };
        Ok(Json(KbReadItemReceipt {
            branch: request.branch,
            item_kind: request.item_kind,
            item_key: request.item_key,
            found,
            content,
            next_hint,
        }))
    }

    /// Create or replace a knowledge-base document (`doc_id`, `name`, `content`) on a target branch.
    ///
    /// Documents with `doc_id`s such as `system_instructions` or `compaction_prompt` are injected into agent preambles or compaction runs. Archived branches reject mutations.
    #[tool(name = "kb_upsert_doc")]
    pub async fn kb_upsert_doc(
        &self,
        Parameters(request): Parameters<KbUpsertDocParams>,
    ) -> Result<Json<KbDocumentReceipt>, ErrorData> {
        let document = self
            .service
            .kb()
            .upsert_document_on(
                &request.branch,
                &request.doc_id,
                &request.name,
                &request.content,
            )
            .await
            .map_err(admin_error)?;
        let next_hint = format!(
            "Document `{}` updated in branch `{}`. Run kb_diff_branch before promotion.",
            document.doc_id, request.branch
        );
        Ok(Json(KbDocumentReceipt {
            document,
            next_hint,
        }))
    }

    /// Create or replace an agent definition (`name`, `model`, `system_prompt`, `allowed_tools`, `is_default`, `enabled`) on a target branch.
    ///
    /// Important constraints:
    /// - `model` must reference an existing route ID registered in `models_list`.
    /// - Setting `is_default: true` automatically clears `is_default` on all other agents in the same branch so exactly one primary orchestrator agent is default.
    /// - Child subagents never receive `spawn_subagents` or `ask_user` at runtime even if listed in `allowed_tools`.
    #[tool(name = "kb_upsert_agent")]
    pub async fn kb_upsert_agent(
        &self,
        Parameters(request): Parameters<KbUpsertAgentParams>,
    ) -> Result<Json<KbAgentReceipt>, ErrorData> {
        let agent = self
            .service
            .kb()
            .upsert_agent_on(
                &request.branch,
                UpsertKbAgentCommand {
                    name: &request.name,
                    description: &request.description,
                    model: &request.model,
                    system_prompt: &request.system_prompt,
                    allowed_tools: &request.allowed_tools,
                    is_default: request.is_default,
                    enabled: request.enabled,
                },
            )
            .await
            .map_err(admin_error)?;
        let next_hint = format!(
            "Agent `{}` updated in branch `{}`. Test it with a session created on that branch.",
            agent.name, request.branch
        );
        Ok(Json(KbAgentReceipt { agent, next_hint }))
    }

    /// Create or replace a skill definition (`name`, `description`, `body`, `enabled`, and bundled `resources`) on a target branch.
    ///
    /// Full-replacement warning:
    /// - `resources` replaces the entire `kb_skill_resources` set for this skill on `branch`.
    /// - When editing an existing skill, always call `kb_read_item(branch, "skill", name)` first and pass back any existing `resources` you intend to keep; omitting `resources` deletes all bundled resource files for that skill.
    #[tool(name = "kb_upsert_skill")]
    pub async fn kb_upsert_skill(
        &self,
        Parameters(request): Parameters<KbUpsertSkillParams>,
    ) -> Result<Json<KbSkillReceipt>, ErrorData> {
        let resources: Vec<(String, String)> = request
            .resources
            .into_iter()
            .map(|resource| (resource.path, resource.content))
            .collect();
        let skill = self
            .service
            .kb()
            .upsert_skill_on(
                &request.branch,
                &request.name,
                &request.description,
                &request.body,
                request.enabled,
                &resources,
            )
            .await
            .map_err(admin_error)?;
        let next_hint = format!(
            "Skill `{}` updated in branch `{}`. Call kb_diff_branch to review the release.",
            skill.name, request.branch
        );
        Ok(Json(KbSkillReceipt { skill, next_hint }))
    }

    /// Delete a knowledge-base item (`doc`, `agent`, or `skill`, cascading to bundled skill resources) from a branch.
    ///
    /// Always pass `branch` explicitly when working in a draft branch; omitting `branch` deletes directly from the current active production branch.
    #[tool(name = "kb_delete_item")]
    pub async fn kb_delete_item(
        &self,
        Parameters(request): Parameters<KbDeleteItemParams>,
    ) -> Result<Json<DeleteReceipt>, ErrorData> {
        let branch = match request.branch {
            Some(branch) => branch,
            None => {
                self.service
                    .kb()
                    .active_branch()
                    .await
                    .map_err(admin_error)?
                    .name
            }
        };
        match request.item_kind {
            KbItemKind::Doc => self
                .service
                .kb()
                .delete_document_on(&branch, &request.item_key)
                .await
                .map_err(admin_error)?,
            KbItemKind::Agent => self
                .service
                .kb()
                .delete_agent_on(&branch, &request.item_key)
                .await
                .map_err(admin_error)?,
            KbItemKind::Skill => self
                .service
                .kb()
                .delete_skill_on(&branch, &request.item_key)
                .await
                .map_err(admin_error)?,
        }
        let target = format!("{branch}:{:?}:{}", request.item_kind, request.item_key);
        let next_hint = format!(
            "Deleted `{}` from branch `{}`. Call kb_list_items to verify.",
            request.item_key, branch
        );
        Ok(Json(DeleteReceipt {
            deleted: true,
            target,
            next_hint,
        }))
    }
}
