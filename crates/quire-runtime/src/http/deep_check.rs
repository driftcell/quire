use crate::auth::{AuthUser, UserRole};
use axum::{
    Extension, Json,
    extract::{Path, State},
};
use chrono::{DateTime, Utc};
use quire_protocol::{
    CommentAuthorType, CommentStatus, DocId, EscalationStatus, EvaluationThreadMeta,
    TextQuoteSelector,
};
use quire_store::NewCommentThread;
use serde::Serialize;

use crate::{
    error::{RuntimeError, RuntimeResult},
    models::build_optional_document_evaluator,
    service::RuntimeService,
};

#[derive(Debug, Clone, Serialize)]
pub(crate) struct DeepCheckStatusView {
    pub doc_id: String,
    pub enabled: bool,
    pub status: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub provider: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub model: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub revision: Option<i64>,
    pub evaluated_blocks: i32,
    pub created_threads: i32,
    pub escalated_count: i32,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub error_message: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub started_at: Option<DateTime<Utc>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub finished_at: Option<DateTime<Utc>>,
}

#[derive(sqlx::FromRow)]
struct DeepCheckRow {
    status: String,
    provider: Option<String>,
    model: Option<String>,
    revision: Option<i64>,
    evaluated_blocks: i32,
    created_threads: i32,
    escalated_count: i32,
    error_message: Option<String>,
    started_at: Option<DateTime<Utc>>,
    finished_at: Option<DateTime<Utc>>,
}

impl DeepCheckRow {
    fn into_view(self, doc_id: &DocId, enabled: bool) -> DeepCheckStatusView {
        DeepCheckStatusView {
            doc_id: doc_id.to_string(),
            enabled,
            status: self.status,
            provider: self.provider,
            model: self.model,
            revision: self.revision,
            evaluated_blocks: self.evaluated_blocks,
            created_threads: self.created_threads,
            escalated_count: self.escalated_count,
            error_message: self.error_message,
            started_at: self.started_at,
            finished_at: self.finished_at,
        }
    }
}

fn idle_view(service: &RuntimeService, doc_id: &DocId) -> DeepCheckStatusView {
    let cfg = &service.config().evaluator;
    DeepCheckStatusView {
        doc_id: doc_id.to_string(),
        enabled: cfg.is_enabled(),
        status: "idle".into(),
        provider: cfg.is_enabled().then(|| cfg.provider.as_str().to_string()),
        model: cfg.is_enabled().then(|| cfg.model.clone()),
        revision: None,
        evaluated_blocks: 0,
        created_threads: 0,
        escalated_count: 0,
        error_message: None,
        started_at: None,
        finished_at: None,
    }
}

async fn load_deep_check_row(
    service: &RuntimeService,
    doc_id: &DocId,
) -> RuntimeResult<Option<DeepCheckRow>> {
    sqlx::query_as::<_, DeepCheckRow>(
        r#"SELECT status, provider, model, revision, evaluated_blocks, created_threads,
                  escalated_count, error_message, started_at, finished_at
           FROM report.deep_checks
           WHERE doc_id = $1"#,
    )
    .bind(doc_id.as_str())
    .fetch_optional(service.store().pool())
    .await
    .map_err(RuntimeError::from)
}

pub(crate) async fn get_deep_check_status(
    State(service): State<RuntimeService>,
    Extension(auth_user): Extension<AuthUser>,
    Path(raw_doc_id): Path<String>,
) -> RuntimeResult<Json<DeepCheckStatusView>> {
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
    let enabled = service.config().evaluator.is_enabled();
    match load_deep_check_row(&service, &doc_id).await? {
        Some(row) => Ok(Json(row.into_view(&doc_id, enabled))),
        None => Ok(Json(idle_view(&service, &doc_id))),
    }
}

pub(crate) async fn trigger_deep_check(
    State(service): State<RuntimeService>,
    Extension(auth_user): Extension<AuthUser>,
    Path(raw_doc_id): Path<String>,
) -> RuntimeResult<Json<DeepCheckStatusView>> {
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
    let eval_cfg = service.config().evaluator.clone();
    let Some(evaluator) = build_optional_document_evaluator(&eval_cfg) else {
        return Err(RuntimeError::InvalidRequest(
            "未启用决策模型：请在环境变量中配置 EVALUATOR_PROVIDER=jev 与 JEV_API_KEY".into(),
        ));
    };

    let report_store = service.kb().store().report_store().clone();
    let document = report_store
        .get_document(&doc_id)
        .await
        .map_err(|e| RuntimeError::Internal(format!("读取报告失败: {e}")))?;

    let provider_str = eval_cfg.provider.as_str().to_string();
    let model_str = eval_cfg.model.clone();
    let revision = document.revision.get();

    // Atomically transition to `running` unless already `running`.
    let inserted = sqlx::query_as::<_, DeepCheckRow>(
        r#"INSERT INTO report.deep_checks
               (doc_id, status, provider, model, revision, evaluated_blocks, created_threads,
                escalated_count, error_message, started_at, finished_at, updated_at)
           VALUES ($1, 'running', $2, $3, $4, 0, 0, 0, NULL, NOW(), NULL, NOW())
           ON CONFLICT (doc_id) DO UPDATE
           SET status = 'running',
               provider = EXCLUDED.provider,
               model = EXCLUDED.model,
               revision = EXCLUDED.revision,
               evaluated_blocks = 0,
               created_threads = 0,
               escalated_count = 0,
               error_message = NULL,
               started_at = NOW(),
               finished_at = NULL,
               updated_at = NOW()
           WHERE report.deep_checks.status <> 'running'
           RETURNING status, provider, model, revision, evaluated_blocks, created_threads,
                     escalated_count, error_message, started_at, finished_at"#,
    )
    .bind(doc_id.as_str())
    .bind(&provider_str)
    .bind(&model_str)
    .bind(revision)
    .fetch_optional(service.store().pool())
    .await?;

    let Some(running_row) = inserted else {
        // Already running: return current persistent status so client stays synced.
        let existing = load_deep_check_row(&service, &doc_id)
            .await?
            .ok_or_else(|| RuntimeError::Internal("deep check state disappeared".into()))?;
        return Ok(Json(existing.into_view(&doc_id, true)));
    };

    let bg_service = service.clone();
    let bg_doc_id = doc_id.clone();
    let bg_provider = provider_str.clone();
    let bg_model = model_str.clone();
    tokio::spawn(async move {
        let outcome = run_deep_check_job(
            &bg_service,
            &bg_doc_id,
            &document,
            evaluator.as_ref(),
            &bg_provider,
            &bg_model,
        )
        .await;

        let pool = bg_service.store().pool();
        match outcome {
            Ok((evaluated_blocks, created_threads, escalated_count)) => {
                let _ = sqlx::query(
                    r#"UPDATE report.deep_checks
                       SET status = 'completed',
                           evaluated_blocks = $2,
                           created_threads = $3,
                           escalated_count = $4,
                           error_message = NULL,
                           finished_at = NOW(),
                           updated_at = NOW()
                       WHERE doc_id = $1"#,
                )
                .bind(bg_doc_id.as_str())
                .bind(evaluated_blocks)
                .bind(created_threads)
                .bind(escalated_count)
                .execute(pool)
                .await;
            }
            Err(err) => {
                let _ = sqlx::query(
                    r#"UPDATE report.deep_checks
                       SET status = 'failed',
                           error_message = $2,
                           finished_at = NOW(),
                           updated_at = NOW()
                       WHERE doc_id = $1"#,
                )
                .bind(bg_doc_id.as_str())
                .bind(err.to_string())
                .execute(pool)
                .await;
            }
        }
    });

    Ok(Json(running_row.into_view(&doc_id, true)))
}

async fn run_deep_check_job(
    service: &RuntimeService,
    doc_id: &DocId,
    document: &quire_protocol::ReportDocument,
    evaluator: &dyn crate::models::DocumentContextualEvaluator,
    provider: &str,
    model: &str,
) -> RuntimeResult<(i32, i32, i32)> {
    let results = evaluator.evaluate_document(document, None).await?;
    let report_store = service.kb().store().report_store();
    let app_state = service.report_app_state();

    let existing_open = report_store
        .list_comment_threads(doc_id, Some(CommentStatus::Open), None)
        .await
        .map_err(|e| RuntimeError::Internal(format!("读取已有批注失败: {e}")))?;

    let evaluated_blocks = i32::try_from(results.len()).unwrap_or(i32::MAX);
    let mut created_threads: i32 = 0;
    let mut escalated_count: i32 = 0;

    for block_res in results {
        for finding in block_res.findings {
            if finding.status == EscalationStatus::AcceptedClean {
                continue;
            }
            let is_escalated = finding.status == EscalationStatus::EscalatedToReasoningLlm;
            if is_escalated {
                escalated_count += 1;
            }

            // Skip if an open evaluator thread for the same (block_id, dimension) already exists.
            let already_exists = existing_open.iter().any(|thread| {
                thread.block_id == finding.block_id
                    && thread
                        .evaluation_meta
                        .as_ref()
                        .is_some_and(|meta| meta.dimension == finding.dimension)
            });
            if already_exists {
                continue;
            }

            let quote = finding
                .quote_exact
                .as_deref()
                .map(str::trim)
                .filter(|q| !q.is_empty())
                .map(|exact| TextQuoteSelector {
                    exact: exact.chars().take(1_800).collect(),
                    prefix: None,
                    suffix: None,
                });

            let dim_label = match finding.dimension.as_str() {
                "information_density" => "信息密度",
                "logical_coherence" => "逻辑连贯",
                "cross_block_data_consistency" => "跨块数据一致",
                "source_groundedness" => "引文支撑度",
                "cross_block_redundancy" => "跨段冗余",
                "section_relevance" => "章节相关性",
                other => other,
            };
            let prefix_tag = if is_escalated {
                format!("[深度检查 · {dim_label} · 待复核]")
            } else {
                format!("[深度检查 · {dim_label}]")
            };
            let body = format!(
                "{prefix_tag} {}\n\n建议：{}",
                finding.summary, finding.suggestion
            );

            let thread = report_store
                .create_comment_thread(
                    doc_id,
                    NewCommentThread {
                        block_id: finding.block_id.clone(),
                        end_block_id: None,
                        anchor_blocks: Vec::new(),
                        author_type: CommentAuthorType::Agent,
                        author: format!("evaluator:{model}"),
                        body,
                        quote,
                        end_quote: None,
                        anchor_snapshot: None,
                        evaluation_meta: Some(EvaluationThreadMeta {
                            provider: provider.to_string(),
                            model: model.to_string(),
                            dimension: finding.dimension,
                            confidence: finding.confidence,
                            score: finding.score,
                            escalated: is_escalated,
                            evidence_fact_id: finding.evidence_fact_id,
                        }),
                    },
                )
                .await
                .map_err(|e| RuntimeError::Internal(format!("写入批注线程失败: {e}")))?;

            app_state.publish_comment_updated(&thread).await;
            created_threads += 1;
        }
    }

    Ok((evaluated_blocks, created_threads, escalated_count))
}
