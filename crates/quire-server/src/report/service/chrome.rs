use super::*;

impl QuireReportService {
    pub async fn exec_set_cover(
        &self,
        context: RequestContext,
        request: SetCoverParams,
    ) -> Result<SetCoverReceipt, ErrorData> {
        let actor = context.actor;
        let doc_id = parse_doc_id(&request.doc_id)?;
        let patch = request.cover_patch;
        let (result, updated) = self
            .apply_with_reload(&doc_id, &actor, "update_cover", |document| {
                let mut cover = document.cover.clone();
                patch.apply_to(&mut cover)?;
                Ok(vec![PatchOp::SetCover {
                    cover: Box::new(cover.clone()),
                }])
            })
            .await?;
        let cover = updated.cover;
        let _next_hint = format!(
            "Cover committed at revision {}. Continue with configure_toc or authoring blocks.",
            result.new_revision
        );
        Ok(SetCoverReceipt {
            doc_id: doc_id.to_string(),
            revision: result.new_revision.get(),
            cover,
            _next_hint,
        })
    }

    pub async fn exec_configure_toc(
        &self,
        context: RequestContext,
        request: ConfigureTocParams,
    ) -> Result<ConfigureTocReceipt, ErrorData> {
        let actor = context.actor;
        if let Some(depth) = request.max_depth
            && !(1..=3).contains(&depth)
        {
            return Err(invalid_params(
                format!("max_depth {depth} is outside 1..=3"),
                "Use max_depth 1, 2 or 3.",
                Some(McpErrorDetails::ValidMaxDepths {
                    valid_max_depths: [1, 2, 3],
                }),
            ));
        }
        if let Some(variant) = &request.variant
            && !TOC_VARIANTS.contains(&variant.as_str())
        {
            return Err(invalid_params(
                format!("unknown TOC variant `{variant}`"),
                format!("Valid TOC variants: {}", TOC_VARIANTS.join(", ")),
                Some(McpErrorDetails::ValidVariants {
                    valid_variants: TOC_VARIANTS,
                }),
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
                if let Some(page_break_before) = request.page_break_before {
                    toc.page_break_before = page_break_before;
                }
                if let Some(page_break_after) = request.page_break_after {
                    toc.page_break_after = page_break_after;
                }
                Ok(vec![PatchOp::SetToc { toc: toc.clone() }])
            })
            .await?;
        let toc = updated.toc;
        let _next_hint = format!(
            "TOC committed at revision {}. Headings are auto-collected by the frontend; do not add TOC blocks.",
            result.new_revision
        );
        Ok(ConfigureTocReceipt {
            doc_id: doc_id.to_string(),
            revision: result.new_revision.get(),
            toc,
            _next_hint,
        })
    }

    pub async fn exec_configure_references(
        &self,
        context: RequestContext,
        request: ConfigureReferencesParams,
    ) -> Result<ConfigureReferencesReceipt, ErrorData> {
        let actor = context.actor;
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
        Ok(ConfigureReferencesReceipt {
            doc_id: doc_id.to_string(),
            revision: result.new_revision.get(),
            references,
            orphan_sources,
            _next_hint,
        })
    }

    pub async fn exec_configure_page_chrome(
        &self,
        context: RequestContext,
        request: ConfigurePageChromeParams,
    ) -> Result<ConfigurePageChromeReceipt, ErrorData> {
        let actor = context.actor;
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
                    Some(McpErrorDetails::CurrentRevision { current_revision }),
                ));
            }
        }
        let (result, updated) = self
            .apply_with_reload(&doc_id, &actor, "configure_page_chrome", |document| {
                let mut theme = document.theme.clone();
                theme.page_chrome = Some(Self::merge_page_chrome(
                    theme.page_chrome.as_ref(),
                    &request,
                ));
                Ok(vec![PatchOp::SetTheme {
                    theme: Box::new(theme),
                }])
            })
            .await?;
        let page_chrome = updated.theme.page_chrome.clone().unwrap_or_default();
        let _next_hint = format!(
            "Page chrome committed at revision {}. Slots support {{page}}, {{total}}, {{title}}, {{subtitle}}, {{date}}, {{kicker}} and {{authors}}; heading_page_break accepts none, h1 or h1_and_h2.",
            result.new_revision
        );
        Ok(ConfigurePageChromeReceipt {
            doc_id: doc_id.to_string(),
            revision: result.new_revision.get(),
            page_chrome,
            _next_hint,
        })
    }

    pub async fn exec_update_style_and_css(
        &self,
        context: RequestContext,
        request: UpdateStyleParams,
    ) -> Result<UpdateStyleReceipt, ErrorData> {
        let actor = context.actor;
        for (name, value) in &request.tokens_patch {
            if !is_valid_css_token_name(name) {
                return Err(invalid_params(
                    format!("invalid CSS token name `{name}`"),
                    "Token names must start with `--` and contain only ASCII letters, digits, '-' or '_'.",
                    Some(McpErrorDetails::ValidExamples {
                        valid_examples: &["--rpt-primary", "--rpt-accent", "--rpt-font-body"],
                    }),
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
                    theme: Box::new(theme.clone()),
                }])
            })
            .await?;
        let theme = updated.theme;
        let _next_hint = format!(
            "Theme committed at revision {}. Run lint_document to re-check CSS and source usage.",
            result.new_revision
        );
        Ok(UpdateStyleReceipt {
            doc_id: doc_id.to_string(),
            revision: result.new_revision.get(),
            theme: theme.summary(),
            token_count: theme.tokens.len(),
            has_custom_css: !theme.custom_css.is_empty(),
            _next_hint,
        })
    }

    pub(crate) fn merge_page_chrome(
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
        if let Some(heading_page_break) = request.heading_page_break {
            chrome.heading_page_break = heading_page_break;
        }
        chrome
    }
}
