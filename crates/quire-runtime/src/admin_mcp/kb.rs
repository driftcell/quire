use super::*;

#[tool_router(router = kb_router, vis = "pub(crate)")]
impl AdminMcpServer {
    // ---------------------------------------------------------------------
    // Branch and release tools
    // ---------------------------------------------------------------------

    #[tool(
        name = "preview_report",
        description = quire_protocol::tool_description!("preview_report")
    )]
    pub async fn preview_report(
        &self,
        Parameters(request): Parameters<crate::preview::PreviewReportInput>,
    ) -> Result<CallToolResult, ErrorData> {
        let result = crate::preview::execute_preview_report(self.service.config(), request)
            .await
            .map_err(admin_error)?;
        Ok(result.to_mcp_call_tool_result())
    }

    #[tool(
        name = "kb_list_branches",
        description = quire_protocol::tool_description!("kb_list_branches")
    )]
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

    #[tool(
        name = "kb_create_branch",
        description = quire_protocol::tool_description!("kb_create_branch")
    )]
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

    #[tool(
        name = "kb_diff_branch",
        description = quire_protocol::tool_description!("kb_diff_branch")
    )]
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

    #[tool(
        name = "kb_promote_branch",
        description = quire_protocol::tool_description!("kb_promote_branch")
    )]
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

    #[tool(
        name = "kb_rollback_active",
        description = quire_protocol::tool_description!("kb_rollback_active")
    )]
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

    #[tool(
        name = "kb_list_items",
        description = quire_protocol::tool_description!("kb_list_items")
    )]
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

    #[tool(
        name = "kb_read_item",
        description = quire_protocol::tool_description!("kb_read_item")
    )]
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

    #[tool(
        name = "kb_upsert_doc",
        description = quire_protocol::tool_description!("kb_upsert_doc")
    )]
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

    #[tool(
        name = "kb_upsert_agent",
        description = quire_protocol::tool_description!("kb_upsert_agent")
    )]
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

    #[tool(
        name = "kb_upsert_skill",
        description = quire_protocol::tool_description!("kb_upsert_skill")
    )]
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

    #[tool(
        name = "kb_delete_item",
        description = quire_protocol::tool_description!("kb_delete_item")
    )]
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

    // ---------------------------------------------------------------------
    // Model and provider registry
    // ---------------------------------------------------------------------
}
