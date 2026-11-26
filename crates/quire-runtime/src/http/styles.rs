use axum::{
    Extension, Json,
    extract::{Path, State},
    http::StatusCode,
};
use quire_protocol::{DocId, PatchOp};
use quire_store::StoreError;
use serde::{Deserialize, Serialize};

use crate::{
    auth::{AuthUser, UserRole},
    error::{RuntimeError, RuntimeResult},
    service::RuntimeService,
    store::{CreateStylePresetInput, StylePresetBundle, StylePresetRecord, UpdateStylePresetInput},
};

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct CreateStyleRequest {
    pub name: String,
    #[serde(default)]
    pub description: String,
    #[serde(default)]
    pub category_tag: String,
    #[serde(default)]
    pub is_public: bool,
    #[serde(default)]
    pub base_preset_id: String,
    pub bundle: StylePresetBundle,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct UpdateStyleRequest {
    #[serde(default)]
    pub name: Option<String>,
    #[serde(default)]
    pub description: Option<String>,
    #[serde(default)]
    pub category_tag: Option<String>,
    #[serde(default)]
    pub is_public: Option<bool>,
    #[serde(default)]
    pub bundle: Option<StylePresetBundle>,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct ApplyStyleToDocRequest {
    #[serde(default)]
    pub style_id: Option<String>,
    #[serde(default)]
    pub bundle: Option<StylePresetBundle>,
    #[serde(default = "default_true")]
    pub sync_layout_variants: bool,
}

const fn default_true() -> bool {
    true
}

#[derive(Debug, Serialize)]
pub(crate) struct ApplyStyleToDocResponse {
    pub doc_id: String,
    pub revision: i64,
    pub preset: String,
}

pub(crate) async fn list_styles(
    State(service): State<RuntimeService>,
    Extension(auth_user): Extension<AuthUser>,
) -> RuntimeResult<Json<Vec<StylePresetRecord>>> {
    service
        .store()
        .list_style_presets_for_user(&auth_user.user_id)
        .await
        .map(Json)
}

pub(crate) async fn create_style(
    State(service): State<RuntimeService>,
    Extension(auth_user): Extension<AuthUser>,
    Json(request): Json<CreateStyleRequest>,
) -> RuntimeResult<Json<StylePresetRecord>> {
    let user = service.store().get_user_by_id(&auth_user.user_id).await?;
    let display_name = user
        .as_ref()
        .map(|u| u.display_name.trim())
        .filter(|s| !s.is_empty())
        .unwrap_or(auth_user.username.as_str());
    let created = service
        .store()
        .create_style_preset(CreateStylePresetInput {
            name: &request.name,
            description: &request.description,
            category_tag: &request.category_tag,
            is_public: request.is_public,
            owner_user_id: &auth_user.user_id,
            owner_display_name: display_name,
            base_preset_id: &request.base_preset_id,
            bundle: request.bundle,
        })
        .await?;
    Ok(Json(created))
}

pub(crate) async fn update_style(
    State(service): State<RuntimeService>,
    Extension(auth_user): Extension<AuthUser>,
    Path(style_id): Path<String>,
    Json(request): Json<UpdateStyleRequest>,
) -> RuntimeResult<Json<StylePresetRecord>> {
    let updated = service
        .store()
        .update_style_preset(
            &style_id,
            &auth_user.user_id,
            auth_user.role == UserRole::Admin,
            UpdateStylePresetInput {
                name: request.name.as_deref(),
                description: request.description.as_deref(),
                category_tag: request.category_tag.as_deref(),
                is_public: request.is_public,
                bundle: request.bundle,
            },
        )
        .await?;
    Ok(Json(updated))
}

pub(crate) async fn delete_style(
    State(service): State<RuntimeService>,
    Extension(auth_user): Extension<AuthUser>,
    Path(style_id): Path<String>,
) -> RuntimeResult<StatusCode> {
    service
        .store()
        .delete_style_preset(
            &style_id,
            &auth_user.user_id,
            auth_user.role == UserRole::Admin,
        )
        .await?;
    Ok(StatusCode::NO_CONTENT)
}

pub(crate) async fn apply_style_to_document(
    State(service): State<RuntimeService>,
    Extension(auth_user): Extension<AuthUser>,
    Path(raw_doc_id): Path<String>,
    Json(request): Json<ApplyStyleToDocRequest>,
) -> RuntimeResult<Json<ApplyStyleToDocResponse>> {
    let doc_id =
        DocId::parse(&raw_doc_id).map_err(|e| RuntimeError::InvalidRequest(e.to_string()))?;
    service
        .store()
        .authorize_document_access(
            doc_id.as_str(),
            None,
            Some(&auth_user.user_id),
            None,
            auth_user.role == UserRole::Admin,
        )
        .await?;

    let resolved_bundle = if let Some(bundle) = request.bundle {
        bundle.validate()?;
        bundle
    } else if let Some(style_id) = request.style_id.as_deref() {
        let record = service
            .store()
            .get_style_preset(style_id)
            .await?
            .ok_or_else(|| RuntimeError::InvalidRequest(format!("样式 `{style_id}` 不存在")))?;
        if !record.is_builtin
            && !record.is_public
            && auth_user.role != UserRole::Admin
            && record.owner_user_id.as_deref() != Some(&auth_user.user_id)
        {
            return Err(RuntimeError::Forbidden("无权使用该私有样式".into()));
        }
        record.bundle
    } else {
        return Err(RuntimeError::InvalidRequest(
            "请提供 style_id 或 bundle".into(),
        ));
    };

    let report_store = service.store().report_store();
    for attempt in 0..4 {
        let document = report_store
            .get_document(&doc_id)
            .await
            .map_err(|e| RuntimeError::InvalidRequest(format!("读取报告失败: {e}")))?;

        let mut ops = Vec::new();
        let mut next_theme = resolved_bundle.theme.clone();
        if next_theme.page_chrome.is_none() {
            next_theme.page_chrome = document.theme.page_chrome.clone();
        }
        ops.push(PatchOp::SetTheme {
            theme: Box::new(next_theme.clone()),
        });

        if request.sync_layout_variants {
            if let Some(cover_variant) = &resolved_bundle.cover_variant {
                let mut cover = document.cover.clone();
                if cover.variant != *cover_variant {
                    cover.variant = cover_variant.clone();
                    ops.push(PatchOp::SetCover {
                        cover: Box::new(cover),
                    });
                }
            }
            if let Some(toc_variant) = &resolved_bundle.toc_variant {
                let mut toc = document.toc.clone();
                if toc.variant != *toc_variant {
                    toc.variant = toc_variant.clone();
                    ops.push(PatchOp::SetToc { toc });
                }
            }
            if let Some(citation_style) = resolved_bundle.citation_style {
                let mut references = document.references.clone();
                if references.style != citation_style {
                    references.style = citation_style;
                    ops.push(PatchOp::SetReferences { references });
                }
            }
        }

        match report_store
            .apply_operations(
                &doc_id,
                document.revision,
                &auth_user.username,
                "apply_style_preset",
                &ops,
            )
            .await
        {
            Ok(outcome) => {
                return Ok(Json(ApplyStyleToDocResponse {
                    doc_id: doc_id.to_string(),
                    revision: outcome.new_revision.get(),
                    preset: next_theme.preset,
                }));
            }
            Err(StoreError::RevisionAhead { .. } | StoreError::Conflict(_)) if attempt < 3 => {
                tokio::task::yield_now().await;
                continue;
            }
            Err(err) => {
                return Err(RuntimeError::Internal(format!("应用样式失败: {err}")));
            }
        }
    }

    Err(RuntimeError::Conflict(
        "报告正在高频并发更新，请稍后重试应用样式".into(),
    ))
}
