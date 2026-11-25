//! Pluggable abstraction layer for contextual document evaluation, Late Chunking embeddings,
//! listwise/cross-encoder reranking, claim-level groundedness verification, and JEV / OpenAI-compatible
//! distillation decision models.
//!
//! ### Environment-driven Optional Activation (Disabled by Default)
//! Set `EVALUATOR_PROVIDER` (or leave unset / `disabled`) to control whether the contextual
//! evaluation engine is active:
//! - **Disabled (Default)**: When `EVALUATOR_PROVIDER` is unset, empty, `"disabled"`, or `"none"`,
//!   or when the required credentials are not provided, `build_optional_document_evaluator()`
//!   returns `Ok(None)` and the runtime runs purely on deterministic static lint (`quire-lint`)
//!   with zero external evaluator overhead.
//! - **`EVALUATOR_PROVIDER=jev` (JEV / OpenAI-compatible Distillation Decision Model)**:
//!   Reads `JEV_API_KEY` (or `EVALUATOR_API_KEY`), optional `JEV_BASE_URL` (or `EVALUATOR_BASE_URL`),
//!   and optional `JEV_DISTILLATION_MODEL` (or `JEV_MODEL` / `EVALUATOR_MODEL`, defaulting to
//!   `"jev-distill"`). Evaluates target paragraphs against the full report context (headings,
//!   paragraphs, table data, ECharts series, and bibliography sources) in one distilled decision
//!   pass to detect vacuous prose (`QR601`), cross-block data contradictions (`QR602`), ungrounded
//!   source claims (`QR603`), cross-block redundancy (`QR604`), topic drift (`QR605`), and logical
//!   coherence breaks (`QR606`).
//! - **`EVALUATOR_PROVIDER=jina` (Jina Late Chunking Embeddings v3/v4 + Listwise Reranker v2/v3)**:
//!   Reads `JINA_API_KEY` (or `EVALUATOR_API_KEY`), optional `JINA_EMBEDDING_MODEL`
//!   (default `"jina-embeddings-v3"`), and `JINA_RERANKER_MODEL` (default `"jina-reranker-v3"`).
//! - **`EVALUATOR_PROVIDER=google` (Google Cloud Discovery Engine Ranking & CheckGrounding)**:
//!   Reads `GOOGLE_EVALUATOR_API_TOKEN` (or `EVALUATOR_API_KEY`) and `GOOGLE_CLOUD_PROJECT`.

use std::{
    collections::BTreeMap,
    env,
    fmt::{Debug, Formatter},
    future::Future,
    pin::Pin,
    sync::Arc,
    time::Duration,
};

use quire_protocol::{
    BlockEvaluationResult, BlockId, ClaimGroundingScore, ContextualBlockSpan,
    ContextualDocumentView, EmbeddingTaskType, EscalationStatus, EvaluationDimension,
    EvaluationFinding, GroundingFact, ReportDocument, SectionEvaluationBatch,
    build_contextual_document_view, partition_section_batches,
};
use secrecy::{ExposeSecret, SecretString};
use serde::{Deserialize, Serialize};
use serde_json::json;

use crate::{
    error::{RuntimeError, RuntimeResult},
    http_client::timed_client,
};

pub const DEFAULT_JEV_DISTILLATION_MODEL: &str = "jev-latest";
pub const DEFAULT_JEV_BASE_URL: &str = "https://api.typesafe.ai";
pub const DEFAULT_EVALUATOR_CONFIDENCE_THRESHOLD: f32 = 0.90;
pub const DEFAULT_JINA_EMBEDDING_MODEL: &str = "jina-embeddings-v3";
pub const DEFAULT_JINA_RERANKER_MODEL: &str = "jina-reranker-v3";
pub const DEFAULT_GOOGLE_RANKING_MODEL: &str = "semantic-ranker-512@latest";

pub type EvaluatorFuture<'a, T> = Pin<Box<dyn Future<Output = RuntimeResult<T>> + Send + 'a>>;

/// Supported decision/evaluator provider kinds configured via `EVALUATOR_PROVIDER`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum EvaluatorProviderKind {
    #[default]
    Disabled,
    /// JEV distillation decision model (`EVALUATOR_PROVIDER=jev`).
    Jev,
    /// Jina Late Chunking + Listwise Reranker (`EVALUATOR_PROVIDER=jina`).
    Jina,
    /// Google Cloud Discovery Engine Ranking + CheckGrounding (`EVALUATOR_PROVIDER=google`).
    Google,
}

impl EvaluatorProviderKind {
    pub fn parse(raw: &str) -> Option<Self> {
        match raw.trim().to_ascii_lowercase().as_str() {
            "" | "disabled" | "none" | "off" | "false" => Some(Self::Disabled),
            "jev" | "jev-distill" | "jev_distill" => Some(Self::Jev),
            "jina" | "jina-ai" => Some(Self::Jina),
            "google" | "google-cloud" | "vertex" | "discovery-engine" => Some(Self::Google),
            _ => None,
        }
    }

    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Disabled => "disabled",
            Self::Jev => "jev",
            Self::Jina => "jina",
            Self::Google => "google",
        }
    }
}

/// Environment-driven configuration for the optional document decision/evaluation model.
#[derive(Clone)]
pub struct EvaluatorConfig {
    pub provider: EvaluatorProviderKind,
    pub api_key: Option<SecretString>,
    pub base_url: Option<String>,
    /// Distillation / judge model name (e.g. `JEV_DISTILLATION_MODEL`, default `"jev-distill"`).
    pub model: String,
    /// Secondary embedding model for composite pipelines (e.g. `"jina-embeddings-v3"`).
    pub embedding_model: String,
    /// Secondary reranker model for composite pipelines (e.g. `"jina-reranker-v3"`).
    pub reranker_model: String,
    /// Google Cloud project ID when `provider == EvaluatorProviderKind::Google`.
    pub google_project_id: Option<String>,
    /// Google Cloud location (default `"global"`).
    pub google_location: String,
    /// Confidence gate `tau` (`0.0..=1.0`, default `0.90`) for Accept-When-Confident / Escalate-When-Unsure.
    pub confidence_threshold: f32,
}

impl Default for EvaluatorConfig {
    fn default() -> Self {
        Self {
            provider: EvaluatorProviderKind::Disabled,
            api_key: None,
            base_url: None,
            model: DEFAULT_JEV_DISTILLATION_MODEL.into(),
            embedding_model: DEFAULT_JINA_EMBEDDING_MODEL.into(),
            reranker_model: DEFAULT_JINA_RERANKER_MODEL.into(),
            google_project_id: None,
            google_location: "global".into(),
            confidence_threshold: DEFAULT_EVALUATOR_CONFIDENCE_THRESHOLD,
        }
    }
}

impl Debug for EvaluatorConfig {
    fn fmt(&self, formatter: &mut Formatter<'_>) -> std::fmt::Result {
        formatter
            .debug_struct("EvaluatorConfig")
            .field("provider", &self.provider)
            .field("api_key", &self.api_key.as_ref().map(|_| "[REDACTED]"))
            .field("base_url", &self.base_url)
            .field("model", &self.model)
            .field("embedding_model", &self.embedding_model)
            .field("reranker_model", &self.reranker_model)
            .field("google_project_id", &self.google_project_id)
            .field("google_location", &self.google_location)
            .field("confidence_threshold", &self.confidence_threshold)
            .finish()
    }
}

fn non_empty_env(keys: &[&str]) -> Option<String> {
    for &key in keys {
        if let Ok(val) = env::var(key) {
            let trimmed = val.trim();
            if !trimmed.is_empty() {
                return Some(trimmed.to_string());
            }
        }
    }
    None
}

impl EvaluatorConfig {
    /// Load evaluator configuration from environment variables.
    ///
    /// If `EVALUATOR_PROVIDER` is unset, empty, or `"disabled"`, or if the required API key is absent,
    /// `is_enabled()` is `false` and the decision model feature stays completely inactive.
    pub fn from_env() -> Self {
        let defaults = Self::default();
        let raw_provider = non_empty_env(&["EVALUATOR_PROVIDER", "DECISION_MODEL_PROVIDER"])
            .unwrap_or_else(|| "disabled".into());
        let provider = EvaluatorProviderKind::parse(&raw_provider).unwrap_or_else(|| {
            tracing::warn!(
                provider = %raw_provider,
                "unknown EVALUATOR_PROVIDER; falling back to disabled"
            );
            EvaluatorProviderKind::Disabled
        });

        let api_key = match provider {
            EvaluatorProviderKind::Disabled => None,
            EvaluatorProviderKind::Jev => {
                non_empty_env(&["JEV_API_KEY", "EVALUATOR_API_KEY"]).map(SecretString::from)
            }
            EvaluatorProviderKind::Jina => {
                non_empty_env(&["JINA_API_KEY", "EVALUATOR_API_KEY"]).map(SecretString::from)
            }
            EvaluatorProviderKind::Google => non_empty_env(&[
                "GOOGLE_EVALUATOR_API_TOKEN",
                "GOOGLE_API_KEY",
                "EVALUATOR_API_KEY",
            ])
            .map(SecretString::from),
        };

        let base_url = match provider {
            EvaluatorProviderKind::Jev => {
                non_empty_env(&["JEV_BASE_URL", "EVALUATOR_BASE_URL", "MODEL_BASE_URL"])
            }
            EvaluatorProviderKind::Jina => non_empty_env(&["JINA_BASE_URL", "EVALUATOR_BASE_URL"]),
            EvaluatorProviderKind::Google => {
                non_empty_env(&["GOOGLE_DISCOVERY_BASE_URL", "EVALUATOR_BASE_URL"])
            }
            EvaluatorProviderKind::Disabled => None,
        };

        let model = non_empty_env(&["JEV_DISTILLATION_MODEL", "JEV_MODEL", "EVALUATOR_MODEL"])
            .unwrap_or(defaults.model);

        let embedding_model = non_empty_env(&["JINA_EMBEDDING_MODEL", "EVALUATOR_EMBEDDING_MODEL"])
            .unwrap_or(defaults.embedding_model);
        let reranker_model = non_empty_env(&["JINA_RERANKER_MODEL", "EVALUATOR_RERANKER_MODEL"])
            .unwrap_or(defaults.reranker_model);
        let google_project_id =
            non_empty_env(&["GOOGLE_CLOUD_PROJECT", "EVALUATOR_GOOGLE_PROJECT"]);
        let google_location =
            non_empty_env(&["GOOGLE_CLOUD_LOCATION", "EVALUATOR_GOOGLE_LOCATION"])
                .unwrap_or(defaults.google_location);
        let confidence_threshold = non_empty_env(&["EVALUATOR_CONFIDENCE_THRESHOLD"])
            .and_then(|v| v.parse::<f32>().ok())
            .map(|v| v.clamp(0.5, 0.99))
            .unwrap_or(defaults.confidence_threshold);

        Self {
            provider,
            api_key,
            base_url,
            model,
            embedding_model,
            reranker_model,
            google_project_id,
            google_location,
            confidence_threshold,
        }
    }

    /// Returns `true` only when an evaluator provider is explicitly selected AND its required
    /// credentials (such as `JEV_API_KEY`) are configured.
    pub fn is_enabled(&self) -> bool {
        match self.provider {
            EvaluatorProviderKind::Disabled => false,
            EvaluatorProviderKind::Jev | EvaluatorProviderKind::Jina => self.api_key.is_some(),
            EvaluatorProviderKind::Google => {
                self.api_key.is_some() && self.google_project_id.is_some()
            }
        }
    }
}

/// Build the optional document evaluator from `EvaluatorConfig`.
/// Returns `None` when disabled or when credentials are omitted, so callers can treat
/// semantic evaluation as a zero-overhead opt-in feature.
pub fn build_optional_document_evaluator(
    config: &EvaluatorConfig,
) -> Option<Arc<dyn DocumentContextualEvaluator>> {
    if !config.is_enabled() {
        return None;
    }
    let api_key = config.api_key.clone()?;
    match config.provider {
        EvaluatorProviderKind::Disabled => None,
        EvaluatorProviderKind::Jev => {
            let mut client = JevDistillationEvaluatorClient::new(api_key, config.model.clone())
                .with_confidence_threshold(config.confidence_threshold);
            if let Some(base_url) = config.base_url.as_ref() {
                client = client.with_base_url(base_url.clone());
            }
            Some(Arc::new(client))
        }
        EvaluatorProviderKind::Jina => {
            let mut jina = JinaEvaluatorClient::new(
                api_key,
                config.embedding_model.clone(),
                config.reranker_model.clone(),
            );
            if let Some(base_url) = config.base_url.as_ref() {
                jina = jina.with_base_url(base_url.clone());
            }
            let pipeline: ContextualEvaluationPipeline<
                JinaEvaluatorClient,
                JinaEvaluatorClient,
                NoopGroundingValidator,
            > = ContextualEvaluationPipeline::new(Some(jina.clone()), Some(jina), None);
            Some(Arc::new(pipeline))
        }
        EvaluatorProviderKind::Google => {
            let project_id = config.google_project_id.clone()?;
            let mut google = GoogleDiscoveryEvaluatorClient::new(
                project_id,
                config.google_location.clone(),
                api_key,
            )
            .with_ranking_model(config.reranker_model.clone());
            if let Some(base_url) = config.base_url.as_ref() {
                google = google.with_base_url(base_url.clone());
            }
            let pipeline: ContextualEvaluationPipeline<
                NoopContextualEmbedder,
                GoogleDiscoveryEvaluatorClient,
                GoogleDiscoveryEvaluatorClient,
            > = ContextualEvaluationPipeline::new(None, Some(google.clone()), Some(google));
            Some(Arc::new(pipeline))
        }
    }
}

/// Request for long-context embeddings with optional Late Chunking.
#[derive(Debug, Clone)]
pub struct LateChunkingEmbeddingRequest<'a> {
    /// Ordered document chunks; when `late_chunking` is true, the upstream model concatenates
    /// all chunks for full-document self-attention before pooling each chunk span.
    pub inputs: &'a [String],
    pub task: EmbeddingTaskType,
    pub late_chunking: bool,
    pub dimensions: Option<u32>,
}

/// One ranked candidate returned by a `RerankerModel`.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct RerankScore {
    /// Original index into the request `documents` slice.
    pub index: usize,
    /// Relevance / quality score (normalized or model-native).
    pub relevance_score: f32,
}

/// Candidate item passed to a `RerankerModel`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct RerankDocument {
    pub id: String,
    pub title: Option<String>,
    pub content: String,
}

/// Trait 1: Long-context embedding model supporting Late Chunking (e.g. `jina-embeddings-v3` / `v4`).
pub trait ContextualEmbedder: Send + Sync {
    fn embed_chunks(
        &self,
        request: LateChunkingEmbeddingRequest<'_>,
    ) -> impl Future<Output = RuntimeResult<Vec<Vec<f32>>>> + Send;
}

/// Trait 2: Cross-encoder or listwise reranker (e.g. `jina-reranker-v3`, Google Discovery Engine Ranking API).
pub trait RerankerModel: Send + Sync {
    fn rerank(
        &self,
        query: &str,
        documents: &[RerankDocument],
        top_n: Option<usize>,
    ) -> impl Future<Output = RuntimeResult<Vec<RerankScore>>> + Send;
}

/// Trait 3: Claim-level groundedness and cross-block consistency checker (e.g. Google CheckGrounding API).
pub trait GroundingValidator: Send + Sync {
    fn check_grounding(
        &self,
        candidate_prose: &str,
        facts: &[GroundingFact],
    ) -> impl Future<Output = RuntimeResult<(f32, Vec<ClaimGroundingScore>)>> + Send;
}

/// Object-safe high-level facade that evaluates all (or selected) prose blocks of a `ReportDocument`
/// with full-document context.
pub trait DocumentContextualEvaluator: Send + Sync {
    fn evaluate_document<'a>(
        &'a self,
        document: &'a ReportDocument,
        target_blocks: Option<&'a [BlockId]>,
    ) -> EvaluatorFuture<'a, Vec<BlockEvaluationResult>>;
}

pub struct NoopContextualEmbedder;

impl ContextualEmbedder for NoopContextualEmbedder {
    async fn embed_chunks(
        &self,
        _request: LateChunkingEmbeddingRequest<'_>,
    ) -> RuntimeResult<Vec<Vec<f32>>> {
        Ok(Vec::new())
    }
}

pub struct NoopGroundingValidator;

impl GroundingValidator for NoopGroundingValidator {
    async fn check_grounding(
        &self,
        _candidate_prose: &str,
        _facts: &[GroundingFact],
    ) -> RuntimeResult<(f32, Vec<ClaimGroundingScore>)> {
        Ok((1.0, Vec::new()))
    }
}

/// Compute cosine similarity between two embedding vectors.
pub fn cosine_similarity(a: &[f32], b: &[f32]) -> f32 {
    if a.len() != b.len() || a.is_empty() {
        return 0.0;
    }
    let mut dot = 0.0_f32;
    let mut norm_a = 0.0_f32;
    let mut norm_b = 0.0_f32;
    for (&x, &y) in a.iter().zip(b.iter()) {
        dot += x * y;
        norm_a += x * x;
        norm_b += y * y;
    }
    let denom = norm_a.sqrt() * norm_b.sqrt();
    if denom <= f32::EPSILON {
        0.0
    } else {
        (dot / denom).clamp(-1.0, 1.0)
    }
}

/// Blanket adapter so any native `rig::DynModel<rig::operation::Rerank>` can be used directly
/// as a `RerankerModel` inside `ContextualEvaluationPipeline`.
impl RerankerModel for rig::DynModel<rig::operation::Rerank> {
    async fn rerank(
        &self,
        query: &str,
        documents: &[RerankDocument],
        top_n: Option<usize>,
    ) -> RuntimeResult<Vec<RerankScore>> {
        if documents.is_empty() {
            return Ok(Vec::new());
        }
        let req = rig::operation::RerankRequest {
            query: query.to_string(),
            documents: documents.iter().map(|doc| doc.content.clone()).collect(),
        };
        let resp = self
            .call(req)
            .await
            .map_err(|error| RuntimeError::Internal(format!("rig rerank call failed: {error}")))?;
        Ok(resp
            .results
            .into_iter()
            .take(top_n.unwrap_or(usize::MAX))
            .map(|r| RerankScore {
                index: r.index,
                relevance_score: r.relevance_score as f32,
            })
            .collect())
    }
}

// ============================================================================
// JEV Distillation Decision Model Backend (`EVALUATOR_PROVIDER=jev`)
// ============================================================================

/// Native TypeSafe System One (`POST /v1/systemone`) decision client for `EVALUATOR_PROVIDER=jev`.
///
/// Follows TypeSafe's official Speculative Fan-Out, Companion Noul, and Composite Scoring patterns
/// alongside the confidence-gated cascade from *JEV-as-a-Judge* (`arXiv:2609.26550`):
/// 1. Partitions the report by section so `state` stays compact (avoiding Context Rot and respecting
///    the `32k per-question` / `64k per-request` limits).
/// 2. Packs all `Score`, `Noul`, and `Choice` questions into one parallel `/v1/systemone` request
///    per section (`state` is billed once at `$0.042 / 1M` input tokens; output tokens are free).
/// 3. Applies confidence gating (`tau`, default `0.90`): high-confidence passes are accepted,
///    high-confidence defects emit anchored `EvaluationFinding`s, and low-confidence items are
///    marked `EscalatedToReasoningLlm` for review by a reasoning LLM.
#[derive(Clone)]
pub struct JevDistillationEvaluatorClient {
    base_url: String,
    api_key: SecretString,
    model: String,
    confidence_threshold: f32,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SystemOneRequest {
    pub model: String,
    pub state: serde_json::Value,
    pub questions: BTreeMap<String, SystemOneQuestion>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum SystemOneQuestion {
    Noul {
        instructions: serde_json::Value,
        #[serde(skip_serializing_if = "Option::is_none")]
        criteria: Option<NoulCriteria>,
    },
    Choice {
        instructions: serde_json::Value,
        criteria: BTreeMap<String, String>,
    },
    Score {
        instructions: serde_json::Value,
        criteria: Vec<String>,
    },
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct NoulCriteria {
    #[serde(rename = "true")]
    pub true_desc: String,
    #[serde(rename = "false")]
    pub false_desc: String,
}

#[derive(Debug, Clone, Deserialize)]
pub struct SystemOneResponse {
    #[serde(default)]
    pub model: String,
    #[serde(default)]
    pub answers: BTreeMap<String, SystemOneAnswer>,
    #[serde(default)]
    pub usage: Option<SystemOneUsage>,
}

#[derive(Debug, Clone, Deserialize)]
pub struct SystemOneUsage {
    #[serde(default)]
    pub input_tokens: u64,
    #[serde(default)]
    pub output_tokens: u64,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum SystemOneAnswer {
    Noul {
        noul: f32,
    },
    Choice {
        choice: String,
        #[serde(default)]
        probabilities: BTreeMap<String, f32>,
        #[serde(default)]
        confidence: Option<f32>,
    },
    Score {
        score: f32,
        #[serde(default)]
        probabilities: BTreeMap<String, f32>,
        #[serde(default)]
        confidence: Option<f32>,
    },
}

impl JevDistillationEvaluatorClient {
    pub fn new(api_key: SecretString, model: impl Into<String>) -> Self {
        Self {
            base_url: DEFAULT_JEV_BASE_URL.into(),
            api_key,
            model: model.into(),
            confidence_threshold: DEFAULT_EVALUATOR_CONFIDENCE_THRESHOLD,
        }
    }

    pub fn with_base_url(mut self, base_url: impl Into<String>) -> Self {
        self.base_url = base_url.into().trim_end_matches('/').to_string();
        self
    }

    pub fn with_confidence_threshold(mut self, tau: f32) -> Self {
        self.confidence_threshold = tau.clamp(0.5, 0.99);
        self
    }

    pub fn model(&self) -> &str {
        &self.model
    }

    pub fn base_url(&self) -> &str {
        &self.base_url
    }

    pub fn confidence_threshold(&self) -> f32 {
        self.confidence_threshold
    }

    /// Build a single Speculative Fan-Out `POST /v1/systemone` request for one section batch.
    pub fn build_system_one_request(&self, batch: &SectionEvaluationBatch) -> SystemOneRequest {
        let sentence_map: BTreeMap<String, serde_json::Value> = batch
            .sentences
            .iter()
            .map(|s| {
                (
                    s.sentence_id.clone(),
                    json!({
                        "block_id": s.block_id.to_string(),
                        "text": s.exact_text,
                    }),
                )
            })
            .collect();

        let block_entries: Vec<serde_json::Value> = batch
            .blocks
            .iter()
            .map(|b| {
                json!({
                    "block_id": b.block_id.to_string(),
                    "text": b.rendered_text,
                    "cited_sources": b.cited_sources.iter().map(|s| s.to_string()).collect::<Vec<_>>(),
                })
            })
            .collect();

        let facts_entries: Vec<serde_json::Value> = batch
            .relevant_facts
            .iter()
            .map(|f| {
                json!({
                    "fact_id": f.fact_id,
                    "title": f.title,
                    "fact_text": f.fact_text,
                })
            })
            .collect();

        let state = json!({
            "section": batch.section_path,
            "document_outline": batch.document_outline,
            "blocks": block_entries,
            "sentences": sentence_map,
            "grounding_facts": facts_entries,
        });

        let mut questions = BTreeMap::new();
        for block in &batch.blocks {
            let bid = block.block_id.to_string();
            let block_sentence_ids: Vec<String> = batch
                .sentences
                .iter()
                .filter(|s| s.block_id == block.block_id)
                .map(|s| s.sentence_id.clone())
                .collect();

            questions.insert(
                format!("{bid}::density"),
                SystemOneQuestion::Score {
                    instructions: json!({
                        "target_block_id": bid,
                        "question": "Rate the empirical information density of `target_block_id` relative to the section."
                    }),
                    criteria: vec![
                        "0: Purely generic AI filler or vague transitional boilerplate with no concrete entities or evidence.".into(),
                        "1: Mostly broad summary statements with minimal specific metrics or mechanisms.".into(),
                        "2: Solid analytical prose containing specific entities, mechanisms, or verifiable claims.".into(),
                        "3: Dense, evidence-backed institutional analysis with precise comparisons or cited metrics.".into(),
                    ],
                },
            );

            questions.insert(
                format!("{bid}::coherence"),
                SystemOneQuestion::Score {
                    instructions: json!({
                        "target_block_id": bid,
                        "question": "Rate the logical coherence and causal soundness of `target_block_id` within `section`."
                    }),
                    criteria: vec![
                        "0: Clear internal contradiction or non-sequitur against adjacent blocks.".into(),
                        "1: Abrupt causal leap or missing premise.".into(),
                        "2: Generally coherent with minor transitional roughness.".into(),
                        "3: Tight, well-supported logical progression aligned with adjacent blocks.".into(),
                    ],
                },
            );

            if !batch.relevant_facts.is_empty() {
                let mut grounding_criteria = BTreeMap::new();
                grounding_criteria.insert(
                    "supported".into(),
                    "Every factual or numerical claim in the target block is consistent with `grounding_facts`.".into(),
                );
                grounding_criteria.insert(
                    "contradicted".into(),
                    "At least one figure, ranking, or directional claim conflicts with `grounding_facts`.".into(),
                );
                grounding_criteria.insert(
                    "unsupported".into(),
                    "The block makes strong factual/numerical claims not established by `grounding_facts`.".into(),
                );
                questions.insert(
                    format!("{bid}::grounding"),
                    SystemOneQuestion::Choice {
                        instructions: json!({
                            "target_block_id": bid,
                            "question": "Assess the relation of factual and numerical claims in `target_block_id` to `grounding_facts` only."
                        }),
                        criteria: grounding_criteria,
                    },
                );
            }

            if !block_sentence_ids.is_empty() {
                questions.insert(
                    format!("{bid}::has_defect"),
                    SystemOneQuestion::Noul {
                        instructions: json!({
                            "target_block_id": bid,
                            "question": "Does `target_block_id` contain a concrete defect (vacuous filler, data contradiction, ungrounded claim, or logical gap)?"
                        }),
                        criteria: Some(NoulCriteria {
                            true_desc: "Contains at least one clear factual conflict, unsupported claim, logical gap, or vacuous boilerplate.".into(),
                            false_desc: "Substantive, logically sound, and consistent with surrounding context and facts.".into(),
                        }),
                    },
                );
                let mut sentence_criteria = BTreeMap::new();
                for sid in &block_sentence_ids {
                    sentence_criteria.insert(
                        sid.clone(),
                        format!("Sentence `{sid}` is the primary source of any defect in `{bid}`."),
                    );
                }
                questions.insert(
                    format!("{bid}::anchor_sentence"),
                    SystemOneQuestion::Choice {
                        instructions: json!({
                            "target_block_id": bid,
                            "question": "Which sentence ID in `target_block_id` most needs revision or verification?"
                        }),
                        criteria: sentence_criteria,
                    },
                );
            }
        }

        SystemOneRequest {
            model: self.model.clone(),
            state,
            questions,
        }
    }

    /// Parse `/v1/systemone` answers into `BlockEvaluationResult`s using confidence-gated routing (`tau`).
    pub fn parse_system_one_response(
        &self,
        batch: &SectionEvaluationBatch,
        response: &SystemOneResponse,
    ) -> Vec<BlockEvaluationResult> {
        let tau = self.confidence_threshold;
        let sentence_lookup: BTreeMap<&str, &str> = batch
            .sentences
            .iter()
            .map(|s| (s.sentence_id.as_str(), s.exact_text.as_str()))
            .collect();
        let default_fact_id = batch.relevant_facts.first().map(|f| f.fact_id.clone());

        let mut results = Vec::with_capacity(batch.blocks.len());
        for block in &batch.blocks {
            let bid = block.block_id.to_string();
            let mut scores = BTreeMap::new();
            let mut findings = Vec::new();

            let anchor_quote = response
                .answers
                .get(&format!("{bid}::anchor_sentence"))
                .and_then(|ans| match ans {
                    SystemOneAnswer::Choice { choice, .. } => sentence_lookup
                        .get(choice.as_str())
                        .map(|s| (*s).to_string()),
                    _ => None,
                });

            let has_defect_prob = response
                .answers
                .get(&format!("{bid}::has_defect"))
                .and_then(|ans| match ans {
                    SystemOneAnswer::Noul { noul } => Some(*noul),
                    _ => None,
                });

            if let Some(SystemOneAnswer::Score {
                score,
                probabilities,
                confidence,
            }) = response.answers.get(&format!("{bid}::density"))
            {
                let norm = (*score / 3.0).clamp(0.0, 1.0);
                scores.insert(EvaluationDimension::InformationDensity, norm);
                let q = max_prob(probabilities).max(confidence.unwrap_or(0.0));
                if norm < 0.45 {
                    let status = if q >= tau {
                        EscalationStatus::FlaggedConfident
                    } else {
                        EscalationStatus::EscalatedToReasoningLlm
                    };
                    findings.push(EvaluationFinding {
                        block_id: block.block_id.clone(),
                        dimension: EvaluationDimension::InformationDensity.as_str().into(),
                        quote_exact: anchor_quote.clone(),
                        summary: format!(
                            "信息密度偏低（归一化得分 {norm:.2}，置信度 {q:.2}），可能存在空泛套话或缺乏实证支撑。"
                        ),
                        suggestion: "补充具体机构对比、指标差异或可核查机制，替换泛化过渡句。".into(),
                        confidence: q,
                        score: Some(norm),
                        status,
                        evidence_fact_id: None,
                    });
                } else if q < tau && has_defect_prob.is_some_and(|p| p >= 0.35) {
                    findings.push(EvaluationFinding {
                        block_id: block.block_id.clone(),
                        dimension: EvaluationDimension::InformationDensity.as_str().into(),
                        quote_exact: anchor_quote.clone(),
                        summary: format!(
                            "决策模型对该段信息密度置信度不足（q={q:.2} < {tau:.2}），建议结合上下文复核。"
                        ),
                        suggestion: "复核本段是否存在隐性空洞或表述冗余。".into(),
                        confidence: q,
                        score: Some(norm),
                        status: EscalationStatus::EscalatedToReasoningLlm,
                        evidence_fact_id: None,
                    });
                }
            }

            if let Some(SystemOneAnswer::Score {
                score,
                probabilities,
                confidence,
            }) = response.answers.get(&format!("{bid}::coherence"))
            {
                let norm = (*score / 3.0).clamp(0.0, 1.0);
                scores.insert(EvaluationDimension::LogicalCoherence, norm);
                let q = max_prob(probabilities).max(confidence.unwrap_or(0.0));
                if norm < 0.50 {
                    let status = if q >= tau {
                        EscalationStatus::FlaggedConfident
                    } else {
                        EscalationStatus::EscalatedToReasoningLlm
                    };
                    findings.push(EvaluationFinding {
                        block_id: block.block_id.clone(),
                        dimension: EvaluationDimension::LogicalCoherence.as_str().into(),
                        quote_exact: anchor_quote.clone(),
                        summary: format!(
                            "段落逻辑连贯性偏弱（归一化得分 {norm:.2}，置信度 {q:.2}），可能存在因果跳跃或上下文断层。"
                        ),
                        suggestion: "补全前提条件与因果链条，确保与相邻段落论证承接自然。".into(),
                        confidence: q,
                        score: Some(norm),
                        status,
                        evidence_fact_id: None,
                    });
                }
            }

            if let Some(SystemOneAnswer::Choice {
                choice,
                probabilities,
                confidence,
            }) = response.answers.get(&format!("{bid}::grounding"))
            {
                let q = max_prob(probabilities).max(confidence.unwrap_or(0.0));
                let p_supported = probabilities.get("supported").copied().unwrap_or(0.0);
                scores.insert(EvaluationDimension::SourceGroundedness, p_supported);
                if choice == "contradicted" {
                    scores.insert(EvaluationDimension::CrossBlockDataConsistency, 1.0 - q);
                    let status = if q >= tau {
                        EscalationStatus::FlaggedConfident
                    } else {
                        EscalationStatus::EscalatedToReasoningLlm
                    };
                    findings.push(EvaluationFinding {
                        block_id: block.block_id.clone(),
                        dimension: EvaluationDimension::CrossBlockDataConsistency
                            .as_str()
                            .into(),
                        quote_exact: anchor_quote.clone(),
                        summary: format!(
                            "检测到正文表述与同节图表或引用事实存在冲突（Jev 判定 `contradicted`，置信度 {q:.2}）。"
                        ),
                        suggestion: "核对本段数字、排名或趋势方向是否与同节表格/ECharts 及引用来源一致。".into(),
                        confidence: q,
                        score: Some(1.0 - q),
                        status,
                        evidence_fact_id: default_fact_id.clone(),
                    });
                } else if choice == "unsupported" || (choice == "supported" && q < tau) {
                    let status = if choice == "unsupported" && q >= tau {
                        EscalationStatus::FlaggedConfident
                    } else {
                        EscalationStatus::EscalatedToReasoningLlm
                    };
                    findings.push(EvaluationFinding {
                        block_id: block.block_id.clone(),
                        dimension: EvaluationDimension::SourceGroundedness.as_str().into(),
                        quote_exact: anchor_quote.clone(),
                        summary: if choice == "unsupported" {
                            format!("该段关键论断缺乏引用事实支撑（Jev 判定 `unsupported`，置信度 {q:.2}）。")
                        } else {
                            format!("事实核验置信度低于阈值（q={q:.2} < {tau:.2}），建议复核证据对齐。")
                        },
                        suggestion: "核查所引 `[^source_id]` 原始证据或收窄未经证实的断言。".into(),
                        confidence: q,
                        score: Some(p_supported),
                        status,
                        evidence_fact_id: default_fact_id.clone(),
                    });
                }
            }

            results.push(BlockEvaluationResult {
                block_id: block.block_id.clone(),
                scores,
                most_similar_block: None,
                claims: Vec::new(),
                findings,
            });
        }
        results
    }
}

fn max_prob(probs: &BTreeMap<String, f32>) -> f32 {
    probs.values().copied().fold(0.0_f32, f32::max)
}

impl DocumentContextualEvaluator for JevDistillationEvaluatorClient {
    fn evaluate_document<'a>(
        &'a self,
        document: &'a ReportDocument,
        target_blocks: Option<&'a [BlockId]>,
    ) -> EvaluatorFuture<'a, Vec<BlockEvaluationResult>> {
        Box::pin(async move {
            let view = build_contextual_document_view(document);
            let batches = partition_section_batches(&view, target_blocks);
            if batches.is_empty() {
                return Ok(Vec::new());
            }

            let client = timed_client(Duration::from_secs(45));
            let url = format!("{}/v1/systemone", self.base_url);
            let mut all_results = Vec::new();

            for batch in &batches {
                if batch.blocks.is_empty() {
                    continue;
                }
                let payload = self.build_system_one_request(batch);
                let response = client
                    .post(&url)
                    .bearer_auth(self.api_key.expose_secret())
                    .json(&payload)
                    .send()
                    .await
                    .map_err(|error| {
                        RuntimeError::Internal(format!("JEV System One request failed: {error}"))
                    })?;
                if !response.status().is_success() {
                    let status = response.status();
                    let body = response.text().await.unwrap_or_default();
                    return Err(RuntimeError::Internal(format!(
                        "JEV System One HTTP {status}: {body}"
                    )));
                }
                let parsed: SystemOneResponse = response.json().await.map_err(|error| {
                    RuntimeError::Internal(format!(
                        "JEV System One response decode failed: {error}"
                    ))
                })?;
                all_results.extend(self.parse_system_one_response(batch, &parsed));
            }

            Ok(all_results)
        })
    }
}

// ============================================================================
// Jina AI Backend (Late Chunking Embeddings v3/v4 + Listwise Reranker v2/v3)
// ============================================================================

#[derive(Debug, Clone)]
pub struct JinaEvaluatorClient {
    base_url: String,
    api_key: SecretString,
    embedding_model: String,
    reranker_model: String,
}

#[derive(Serialize)]
struct JinaEmbeddingWireRequest<'a> {
    model: &'a str,
    input: &'a [String],
    task: &'a str,
    late_chunking: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    dimensions: Option<u32>,
}

#[derive(Deserialize)]
struct JinaEmbeddingWireResponse {
    data: Vec<JinaEmbeddingDatum>,
}

#[derive(Deserialize)]
struct JinaEmbeddingDatum {
    index: usize,
    embedding: Vec<f32>,
}

#[derive(Serialize)]
struct JinaRerankWireRequest<'a> {
    model: &'a str,
    query: &'a str,
    documents: Vec<&'a str>,
    #[serde(skip_serializing_if = "Option::is_none")]
    top_n: Option<usize>,
    return_documents: bool,
}

#[derive(Deserialize)]
struct JinaRerankWireResponse {
    results: Vec<RerankScore>,
}

impl JinaEvaluatorClient {
    pub fn new(
        api_key: SecretString,
        embedding_model: impl Into<String>,
        reranker_model: impl Into<String>,
    ) -> Self {
        Self {
            base_url: "https://api.jina.ai/v1".into(),
            api_key,
            embedding_model: embedding_model.into(),
            reranker_model: reranker_model.into(),
        }
    }

    pub fn with_base_url(mut self, base_url: impl Into<String>) -> Self {
        self.base_url = base_url.into().trim_end_matches('/').to_string();
        self
    }
}

impl ContextualEmbedder for JinaEvaluatorClient {
    async fn embed_chunks(
        &self,
        request: LateChunkingEmbeddingRequest<'_>,
    ) -> RuntimeResult<Vec<Vec<f32>>> {
        if request.inputs.is_empty() {
            return Ok(Vec::new());
        }
        let client = timed_client(Duration::from_secs(60));
        let url = format!("{}/embeddings", self.base_url);
        let wire = JinaEmbeddingWireRequest {
            model: &self.embedding_model,
            input: request.inputs,
            task: request.task.jina_task_str(),
            late_chunking: request.late_chunking,
            dimensions: request.dimensions,
        };
        let response = client
            .post(&url)
            .bearer_auth(self.api_key.expose_secret())
            .json(&wire)
            .send()
            .await
            .map_err(|error| RuntimeError::Internal(format!("Jina embeddings request: {error}")))?;
        if !response.status().is_success() {
            let status = response.status();
            let body = response.text().await.unwrap_or_default();
            return Err(RuntimeError::Internal(format!(
                "Jina embeddings HTTP {status}: {body}"
            )));
        }
        let mut parsed: JinaEmbeddingWireResponse = response
            .json()
            .await
            .map_err(|error| RuntimeError::Internal(format!("Jina embeddings decode: {error}")))?;
        parsed.data.sort_by_key(|item| item.index);
        Ok(parsed.data.into_iter().map(|item| item.embedding).collect())
    }
}

impl RerankerModel for JinaEvaluatorClient {
    async fn rerank(
        &self,
        query: &str,
        documents: &[RerankDocument],
        top_n: Option<usize>,
    ) -> RuntimeResult<Vec<RerankScore>> {
        if documents.is_empty() {
            return Ok(Vec::new());
        }
        let client = timed_client(Duration::from_secs(60));
        let url = format!("{}/rerank", self.base_url);
        let wire = JinaRerankWireRequest {
            model: &self.reranker_model,
            query,
            documents: documents.iter().map(|doc| doc.content.as_str()).collect(),
            top_n,
            return_documents: false,
        };
        let response = client
            .post(&url)
            .bearer_auth(self.api_key.expose_secret())
            .json(&wire)
            .send()
            .await
            .map_err(|error| RuntimeError::Internal(format!("Jina rerank request: {error}")))?;
        if !response.status().is_success() {
            let status = response.status();
            let body = response.text().await.unwrap_or_default();
            return Err(RuntimeError::Internal(format!(
                "Jina rerank HTTP {status}: {body}"
            )));
        }
        let parsed: JinaRerankWireResponse = response
            .json()
            .await
            .map_err(|error| RuntimeError::Internal(format!("Jina rerank decode: {error}")))?;
        Ok(parsed.results)
    }
}

// ============================================================================
// Google Cloud Discovery Engine Backend (Ranking API + CheckGrounding API)
// ============================================================================

#[derive(Debug, Clone)]
pub struct GoogleDiscoveryEvaluatorClient {
    base_url: String,
    project_id: String,
    location: String,
    api_token: SecretString,
    ranking_model: String,
    citation_threshold: f32,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct GoogleRankWireRequest<'a> {
    model: &'a str,
    query: &'a str,
    #[serde(skip_serializing_if = "Option::is_none")]
    top_n: Option<usize>,
    ignore_record_details_in_response: bool,
    records: Vec<GoogleRankRecord<'a>>,
}

#[derive(Serialize)]
struct GoogleRankRecord<'a> {
    id: &'a str,
    #[serde(skip_serializing_if = "Option::is_none")]
    title: Option<&'a str>,
    content: &'a str,
}

#[derive(Deserialize)]
struct GoogleRankWireResponse {
    #[serde(default)]
    records: Vec<GoogleRankedRecord>,
}

#[derive(Deserialize)]
struct GoogleRankedRecord {
    id: String,
    #[serde(default)]
    score: f32,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct GoogleCheckGroundingWireRequest<'a> {
    answer_candidate: &'a str,
    facts: Vec<GoogleGroundingFactWire<'a>>,
    grounding_spec: GoogleGroundingSpecWire,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct GoogleGroundingFactWire<'a> {
    fact_text: &'a str,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct GoogleGroundingSpecWire {
    citation_threshold: f32,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct GoogleCheckGroundingWireResponse {
    #[serde(default)]
    support_score: f32,
    #[serde(default)]
    claims: Vec<GoogleGroundingClaimWire>,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct GoogleGroundingClaimWire {
    #[serde(default)]
    claim_text: String,
    #[serde(default)]
    fact_indices: Vec<usize>,
    #[serde(default)]
    score: Option<f32>,
}

impl GoogleDiscoveryEvaluatorClient {
    pub fn new(
        project_id: impl Into<String>,
        location: impl Into<String>,
        api_token: SecretString,
    ) -> Self {
        Self {
            base_url: "https://discoveryengine.googleapis.com/v1".into(),
            project_id: project_id.into(),
            location: location.into(),
            api_token,
            ranking_model: DEFAULT_GOOGLE_RANKING_MODEL.into(),
            citation_threshold: 0.6,
        }
    }

    pub fn with_base_url(mut self, base_url: impl Into<String>) -> Self {
        self.base_url = base_url.into().trim_end_matches('/').to_string();
        self
    }

    pub fn with_ranking_model(mut self, model: impl Into<String>) -> Self {
        self.ranking_model = model.into();
        self
    }
}

impl RerankerModel for GoogleDiscoveryEvaluatorClient {
    async fn rerank(
        &self,
        query: &str,
        documents: &[RerankDocument],
        top_n: Option<usize>,
    ) -> RuntimeResult<Vec<RerankScore>> {
        if documents.is_empty() {
            return Ok(Vec::new());
        }
        let client = timed_client(Duration::from_secs(60));
        let url = format!(
            "{}/projects/{}/locations/{}/rankingConfigs/default_ranking_config:rank",
            self.base_url, self.project_id, self.location
        );
        let records = documents
            .iter()
            .map(|doc| GoogleRankRecord {
                id: &doc.id,
                title: doc.title.as_deref(),
                content: &doc.content,
            })
            .collect();
        let wire = GoogleRankWireRequest {
            model: &self.ranking_model,
            query,
            top_n,
            ignore_record_details_in_response: true,
            records,
        };
        let response = client
            .post(&url)
            .bearer_auth(self.api_token.expose_secret())
            .json(&wire)
            .send()
            .await
            .map_err(|error| RuntimeError::Internal(format!("Google rank request: {error}")))?;
        if !response.status().is_success() {
            let status = response.status();
            let body = response.text().await.unwrap_or_default();
            return Err(RuntimeError::Internal(format!(
                "Google rank HTTP {status}: {body}"
            )));
        }
        let parsed: GoogleRankWireResponse = response
            .json()
            .await
            .map_err(|error| RuntimeError::Internal(format!("Google rank decode: {error}")))?;
        let index_by_id: BTreeMap<&str, usize> = documents
            .iter()
            .enumerate()
            .map(|(idx, doc)| (doc.id.as_str(), idx))
            .collect();
        Ok(parsed
            .records
            .into_iter()
            .filter_map(|rec| {
                index_by_id.get(rec.id.as_str()).map(|&index| RerankScore {
                    index,
                    relevance_score: rec.score,
                })
            })
            .collect())
    }
}

impl GroundingValidator for GoogleDiscoveryEvaluatorClient {
    async fn check_grounding(
        &self,
        candidate_prose: &str,
        facts: &[GroundingFact],
    ) -> RuntimeResult<(f32, Vec<ClaimGroundingScore>)> {
        if candidate_prose.trim().is_empty() || facts.is_empty() {
            return Ok((1.0, Vec::new()));
        }
        let client = timed_client(Duration::from_secs(60));
        let url = format!(
            "{}/projects/{}/locations/{}/groundingConfigs/default_grounding_config:check",
            self.base_url, self.project_id, self.location
        );
        let wire = GoogleCheckGroundingWireRequest {
            answer_candidate: candidate_prose,
            facts: facts
                .iter()
                .take(200)
                .map(|fact| GoogleGroundingFactWire {
                    fact_text: &fact.fact_text,
                })
                .collect(),
            grounding_spec: GoogleGroundingSpecWire {
                citation_threshold: self.citation_threshold,
            },
        };
        let response = client
            .post(&url)
            .bearer_auth(self.api_token.expose_secret())
            .json(&wire)
            .send()
            .await
            .map_err(|error| {
                RuntimeError::Internal(format!("Google checkGrounding request: {error}"))
            })?;
        if !response.status().is_success() {
            let status = response.status();
            let body = response.text().await.unwrap_or_default();
            return Err(RuntimeError::Internal(format!(
                "Google checkGrounding HTTP {status}: {body}"
            )));
        }
        let parsed: GoogleCheckGroundingWireResponse = response.json().await.map_err(|error| {
            RuntimeError::Internal(format!("Google checkGrounding decode: {error}"))
        })?;
        let claims = parsed
            .claims
            .into_iter()
            .map(|claim| {
                let supporting_facts: Vec<String> = claim
                    .fact_indices
                    .iter()
                    .filter_map(|&idx| facts.get(idx).map(|fact| fact.fact_id.clone()))
                    .collect();
                let default_score = if supporting_facts.is_empty() {
                    0.0
                } else {
                    parsed.support_score
                };
                let support_score = claim.score.unwrap_or(default_score);
                let contradiction_note = (support_score < self.citation_threshold).then(|| {
                    format!(
                        "Claim `{}` is not supported by document tables/charts/sources (score {support_score:.2})",
                        claim.claim_text
                    )
                });
                ClaimGroundingScore {
                    claim_text: claim.claim_text,
                    support_score,
                    supporting_facts,
                    contradiction_note,
                }
            })
            .collect();
        Ok((parsed.support_score, claims))
    }
}

// ============================================================================
// Composite Pipeline: Late Chunking + Reranker + Grounding Validator
// ============================================================================

/// Combines optional Late Chunking embeddings, listwise reranking, and claim grounding
/// to evaluate every target paragraph in the context of the entire report.
pub struct ContextualEvaluationPipeline<E, R, G> {
    pub embedder: Option<E>,
    pub reranker: Option<R>,
    pub grounding: Option<G>,
}

impl<E, R, G> ContextualEvaluationPipeline<E, R, G> {
    pub const fn new(embedder: Option<E>, reranker: Option<R>, grounding: Option<G>) -> Self {
        Self {
            embedder,
            reranker,
            grounding,
        }
    }
}

impl<E, R, G> DocumentContextualEvaluator for ContextualEvaluationPipeline<E, R, G>
where
    E: ContextualEmbedder + 'static,
    R: RerankerModel + 'static,
    G: GroundingValidator + 'static,
{
    fn evaluate_document<'a>(
        &'a self,
        document: &'a ReportDocument,
        target_blocks: Option<&'a [BlockId]>,
    ) -> EvaluatorFuture<'a, Vec<BlockEvaluationResult>> {
        Box::pin(async move {
            let view: ContextualDocumentView = build_contextual_document_view(document);
            if view.spans.is_empty() {
                return Ok(Vec::new());
            }

            // Step 1: Full-document Late Chunking embeddings across all blocks in order.
            let chunk_embeddings = if let Some(embedder) = self.embedder.as_ref() {
                let inputs: Vec<String> = view
                    .spans
                    .iter()
                    .map(|span| span.rendered_text.clone())
                    .collect();
                Some(
                    embedder
                        .embed_chunks(LateChunkingEmbeddingRequest {
                            inputs: &inputs,
                            task: EmbeddingTaskType::TextMatching,
                            late_chunking: true,
                            dimensions: None,
                        })
                        .await?,
                )
            } else {
                None
            };

            // Step 2: Listwise reranking of prose blocks against the document title / thesis.
            let prose_indices: Vec<usize> = view
                .spans
                .iter()
                .enumerate()
                .filter(|(_, span)| {
                    span.is_evaluatable_prose
                        && target_blocks
                            .is_none_or(|targets| targets.iter().any(|id| id == &span.block_id))
                })
                .map(|(idx, _)| idx)
                .collect();

            let mut rerank_scores: BTreeMap<usize, f32> = BTreeMap::new();
            if let Some(reranker) = self.reranker.as_ref()
                && !prose_indices.is_empty()
            {
                let candidates: Vec<RerankDocument> = prose_indices
                    .iter()
                    .filter_map(|&idx| {
                        view.spans.get(idx).map(|span| RerankDocument {
                            id: span.block_id.to_string(),
                            title: span.section_path.last().cloned(),
                            content: span.rendered_text.clone(),
                        })
                    })
                    .collect();
                for scored in reranker.rerank(&view.title, &candidates, None).await? {
                    if let Some(&span_idx) = prose_indices.get(scored.index) {
                        rerank_scores.insert(span_idx, scored.relevance_score);
                    }
                }
            }

            // Step 3: Assemble per-block contextual scores and run claim grounding checks.
            let mut results = Vec::with_capacity(prose_indices.len());
            for &idx in &prose_indices {
                let Some(span) = view.spans.get(idx) else {
                    continue;
                };
                let mut scores = BTreeMap::new();
                let mut most_similar_block: Option<(BlockId, f32)> = None;

                if let Some(embeddings) = chunk_embeddings.as_ref()
                    && let Some(vec_i) = embeddings.get(idx)
                {
                    for (other_idx, other_span) in view.spans.iter().enumerate() {
                        if other_idx == idx || !other_span.is_evaluatable_prose {
                            continue;
                        }
                        if let Some(vec_j) = embeddings.get(other_idx) {
                            let sim = cosine_similarity(vec_i, vec_j);
                            if most_similar_block
                                .as_ref()
                                .is_none_or(|(_, best)| sim > *best)
                            {
                                most_similar_block = Some((other_span.block_id.clone(), sim));
                            }
                        }
                    }
                    let max_red = most_similar_block.as_ref().map_or(0.0, |(_, s)| *s);
                    let citation_bonus = if span.cited_sources.is_empty() {
                        0.0
                    } else {
                        0.15
                    };
                    let density = ((1.0 - max_red * 0.5) + citation_bonus).clamp(0.0, 1.0);
                    scores.insert(EvaluationDimension::InformationDensity, density);
                }

                if let Some(&rel) = rerank_scores.get(&idx) {
                    scores.insert(EvaluationDimension::SectionRelevance, rel.clamp(0.0, 1.0));
                }

                let mut claims = Vec::new();
                if let Some(grounding) = self.grounding.as_ref() {
                    let (support_score, verified_claims) = grounding
                        .check_grounding(&span.rendered_text, &view.grounding_facts)
                        .await?;
                    scores.insert(
                        EvaluationDimension::SourceGroundedness,
                        support_score.clamp(0.0, 1.0),
                    );
                    scores.insert(
                        EvaluationDimension::CrossBlockDataConsistency,
                        support_score.clamp(0.0, 1.0),
                    );
                    claims = verified_claims;
                }

                let mut block_result = BlockEvaluationResult {
                    block_id: span.block_id.clone(),
                    scores,
                    most_similar_block,
                    claims,
                    findings: Vec::new(),
                };
                block_result.synthesize_findings(0.90);
                results.push(block_result);
            }

            Ok(results)
        })
    }
}

pub fn span_slices(view: &ContextualDocumentView) -> &[ContextualBlockSpan] {
    &view.spans
}

#[cfg(test)]
mod tests {
    use super::*;
    use quire_protocol::{Block, BlockKind, DocId, SourceEntry, SourceId};

    struct MockLateChunkingEmbedder;

    impl ContextualEmbedder for MockLateChunkingEmbedder {
        async fn embed_chunks(
            &self,
            request: LateChunkingEmbeddingRequest<'_>,
        ) -> RuntimeResult<Vec<Vec<f32>>> {
            assert!(request.late_chunking);
            Ok(request
                .inputs
                .iter()
                .map(|text| {
                    if text.contains("重复的空洞套话") {
                        vec![1.0, 0.0, 0.0]
                    } else {
                        vec![0.0, 1.0, 0.0]
                    }
                })
                .collect())
        }
    }

    struct MockListwiseReranker;

    impl RerankerModel for MockListwiseReranker {
        async fn rerank(
            &self,
            _query: &str,
            documents: &[RerankDocument],
            _top_n: Option<usize>,
        ) -> RuntimeResult<Vec<RerankScore>> {
            Ok(documents
                .iter()
                .enumerate()
                .map(|(index, doc)| RerankScore {
                    index,
                    relevance_score: if doc.content.contains("跑题") {
                        0.25
                    } else {
                        0.88
                    },
                })
                .collect())
        }
    }

    struct MockGroundingValidator;

    impl GroundingValidator for MockGroundingValidator {
        async fn check_grounding(
            &self,
            candidate_prose: &str,
            _facts: &[GroundingFact],
        ) -> RuntimeResult<(f32, Vec<ClaimGroundingScore>)> {
            if candidate_prose.contains("经费高达999亿元") {
                Ok((
                    0.20,
                    vec![ClaimGroundingScore {
                        claim_text: "经费高达999亿元".into(),
                        support_score: 0.20,
                        supporting_facts: Vec::new(),
                        contradiction_note: Some("与表1的32.4亿元矛盾".into()),
                    }],
                ))
            } else {
                Ok((0.95, Vec::new()))
            }
        }
    }

    #[test]
    fn evaluator_is_disabled_by_default_and_activates_for_jev_with_api_key() {
        let default_cfg = EvaluatorConfig::default();
        assert!(!default_cfg.is_enabled());
        assert!(build_optional_document_evaluator(&default_cfg).is_none());

        let jev_without_key = EvaluatorConfig {
            provider: EvaluatorProviderKind::Jev,
            api_key: None,
            ..EvaluatorConfig::default()
        };
        assert!(!jev_without_key.is_enabled());
        assert!(build_optional_document_evaluator(&jev_without_key).is_none());

        let jev_with_key = EvaluatorConfig {
            provider: EvaluatorProviderKind::Jev,
            api_key: Some(SecretString::from("jev-secret-key")),
            base_url: Some("https://api.jev.example/v1".into()),
            model: "jev-distill-v1".into(),
            ..EvaluatorConfig::default()
        };
        assert!(jev_with_key.is_enabled());
        let debug_str = format!("{jev_with_key:?}");
        assert!(debug_str.contains("[REDACTED]"));
        assert!(!debug_str.contains("jev-secret-key"));
        assert!(build_optional_document_evaluator(&jev_with_key).is_some());
    }

    #[test]
    fn jev_system_one_speculative_fanout_and_confidence_gating_work() {
        let mut doc = ReportDocument::new(DocId::parse("doc_jev").expect("doc id"));
        let sc_id = SourceId::parse("sc-budget").expect("source id");
        doc.sources.insert(
            sc_id.clone(),
            SourceEntry {
                title: "经费决算表".into(),
                url: None,
                authors: Vec::new(),
                publisher: Some("财务处".into()),
                date: Some("2025".into()),
                accessed: None,
                note: Some("年度实际经费为 15.2 亿元".into()),
                excerpt: None,
            },
        );
        doc.blocks.push(Block::new(
            BlockId::parse("h1").expect("block id"),
            BlockKind::Heading {
                level: 1,
                text: "经费投入与结构".into(),
            },
        ));
        let mut p_target = Block::new(
            BlockId::parse("p_target").expect("block id"),
            BlockKind::Paragraph {
                text: "本年度经费达到 99.9 亿元[^sc-budget]。这一成绩充分体现了整体跃升。".into(),
            },
        );
        p_target.source_ids.push(sc_id);
        doc.blocks.push(p_target);

        let view = build_contextual_document_view(&doc);
        let batches = partition_section_batches(&view, None);
        assert_eq!(batches.len(), 1);
        let batch = &batches[0];

        let client = JevDistillationEvaluatorClient::new(
            SecretString::from("test-key"),
            DEFAULT_JEV_DISTILLATION_MODEL,
        )
        .with_confidence_threshold(0.90);
        let req = client.build_system_one_request(batch);
        assert_eq!(req.model, "jev-latest");
        assert!(req.questions.contains_key("p_target::density"));
        assert!(req.questions.contains_key("p_target::coherence"));
        assert!(req.questions.contains_key("p_target::grounding"));
        assert!(req.questions.contains_key("p_target::has_defect"));
        assert!(req.questions.contains_key("p_target::anchor_sentence"));

        let raw_resp = serde_json::json!({
            "model": "jev-1.14.0",
            "answers": {
                "p_target::density": {
                    "type": "score",
                    "score": 0.6,
                    "probabilities": { "0": 0.55, "1": 0.35, "2": 0.10, "3": 0.0 },
                    "confidence": 0.55
                },
                "p_target::coherence": {
                    "type": "score",
                    "score": 2.8,
                    "probabilities": { "0": 0.0, "1": 0.0, "2": 0.05, "3": 0.95 },
                    "confidence": 0.95
                },
                "p_target::grounding": {
                    "type": "choice",
                    "choice": "contradicted",
                    "probabilities": { "supported": 0.02, "contradicted": 0.96, "unsupported": 0.02 },
                    "confidence": 0.96
                },
                "p_target::has_defect": {
                    "type": "noul",
                    "noul": 0.97
                },
                "p_target::anchor_sentence": {
                    "type": "choice",
                    "choice": "s_1",
                    "probabilities": { "s_1": 0.94, "s_2": 0.06 },
                    "confidence": 0.94
                }
            },
            "usage": { "input_tokens": 520, "output_tokens": 18 }
        });
        let parsed: SystemOneResponse = serde_json::from_value(raw_resp).expect("decode response");
        let results = client.parse_system_one_response(batch, &parsed);
        assert_eq!(results.len(), 1);
        let findings = &results[0].findings;
        assert_eq!(findings.len(), 2);

        assert_eq!(findings[0].dimension, "information_density");
        assert_eq!(
            findings[0].status,
            EscalationStatus::EscalatedToReasoningLlm
        );

        assert_eq!(findings[1].dimension, "cross_block_data_consistency");
        assert_eq!(findings[1].status, EscalationStatus::FlaggedConfident);
        assert_eq!(
            findings[1].quote_exact.as_deref(),
            Some("本年度经费达到 99.9 亿元[^sc-budget]。")
        );
        assert_eq!(
            findings[1].evidence_fact_id.as_deref(),
            Some("source:sc-budget")
        );
    }

    #[tokio::test]
    async fn composite_pipeline_detects_redundancy_drift_and_data_contradiction() {
        let mut doc = ReportDocument::new(DocId::parse("doc_pipe").expect("doc id"));
        doc.blocks.push(Block::new(
            BlockId::parse("h1").expect("block id"),
            BlockKind::Heading {
                level: 1,
                text: "经费投入与结构".into(),
            },
        ));
        doc.blocks.push(Block::new(
            BlockId::parse("p_dup_a").expect("block id"),
            BlockKind::Paragraph {
                text: "这是一段重复的空洞套话，缺乏实证内容。".into(),
            },
        ));
        doc.blocks.push(Block::new(
            BlockId::parse("p_dup_b").expect("block id"),
            BlockKind::Paragraph {
                text: "这是另一段重复的空洞套话，并且声称经费高达999亿元，完全跑题。".into(),
            },
        ));

        let pipeline = ContextualEvaluationPipeline::new(
            Some(MockLateChunkingEmbedder),
            Some(MockListwiseReranker),
            Some(MockGroundingValidator),
        );
        let results = pipeline
            .evaluate_document(&doc, None)
            .await
            .expect("pipeline evaluation");
        assert_eq!(results.len(), 2);
        let second = &results[1];
        let dims: Vec<&str> = second
            .findings
            .iter()
            .map(|f| f.dimension.as_str())
            .collect();
        assert!(dims.contains(&"cross_block_data_consistency"));
        assert!(dims.contains(&"source_groundedness"));
        assert!(dims.contains(&"cross_block_redundancy"));
        assert!(dims.contains(&"section_relevance"));
    }
}
