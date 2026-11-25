//! Wire types for contextual semantic evaluation, late chunking, grounding verification,
//! and confidence-gated System One decision models (e.g. TypeSafe Jev `/v1/systemone`).
//!
//! Evaluation findings live outside `quire-lint`'s deterministic `QR001`–`QR4xx` codes and are
//! surfaced as side-channel `CommentThread` records carrying `EvaluationThreadMeta`.

use std::collections::{BTreeMap, BTreeSet};

use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

use crate::{
    BlockId, ReportDocument, SourceId,
    block::BlockKind,
    citation::scan_document_citations,
    markdown::{MarkdownOptions, project_blocks},
};

/// Task intent for task-specific LoRA / contextual embedding models (e.g. `jina-embeddings-v3`).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum EmbeddingTaskType {
    /// Query side of asymmetric retrieval (`retrieval.query`).
    RetrievalQuery,
    /// Document/passage side of asymmetric retrieval (`retrieval.passage`).
    RetrievalPassage,
    /// Symmetric semantic similarity & cross-block redundancy (`text-matching`).
    TextMatching,
    /// Quality or vacuousness classification (`classification`).
    Classification,
    /// Topic clustering and drift detection (`separation`).
    Separation,
}

impl EmbeddingTaskType {
    pub const fn jina_task_str(self) -> &'static str {
        match self {
            Self::RetrievalQuery => "retrieval.query",
            Self::RetrievalPassage => "retrieval.passage",
            Self::TextMatching => "text-matching",
            Self::Classification => "classification",
            Self::Separation => "separation",
        }
    }
}

/// Metadata attached to a `CommentThread` created by a contextual decision/evaluation model.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct EvaluationThreadMeta {
    /// Evaluator provider identifier (`"jev"`, `"jina"`, `"google"`, or `"composite"`).
    pub provider: String,
    /// Exact model ID or alias returned by the provider (e.g. `"jev-1.14.0"`).
    pub model: String,
    /// Open evaluation rubric / dimension key (e.g. `"source_groundedness"`, `"information_density"`).
    pub dimension: String,
    /// Decision confidence (`max_k p_k` or provider-calibrated confidence in `0.0..=1.0`).
    pub confidence: f32,
    /// Optional normalized quality score (`0.0..=1.0`) from ordered `Score` primitives.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub score: Option<f32>,
    /// True when confidence fell below the escalation threshold `tau` and requires reasoning-LLM review.
    pub escalated: bool,
    /// Optional `GroundingFact::fact_id` that triggered or contradicted the claim.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub evidence_fact_id: Option<String>,
}

/// One sentence extracted from a prose block with an exact substring suitable for `TextQuoteSelector`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
pub struct SentenceSpan {
    /// Deterministic per-batch sentence identifier (e.g. `"s_1"`).
    pub sentence_id: String,
    pub block_id: BlockId,
    /// Exact substring inside the block's text so it can serve directly as `TextQuoteSelector::exact`.
    pub exact_text: String,
}

/// One block projected into a full-document contextual stream with byte-offset boundary cues.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
pub struct ContextualBlockSpan {
    pub block_id: BlockId,
    /// Active heading path enclosing this block, e.g. `["一、学科建设", "（一）高原与高峰结构"]`.
    pub section_path: Vec<String>,
    /// Canonical readable projection of the block (tables/charts projected as structured data).
    pub rendered_text: String,
    /// Original unexpanded prose text for `Paragraph`/`Quote` blocks (used for exact sentence anchoring).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub raw_text: Option<String>,
    /// Byte span `(start_byte, end_byte)` inside `ContextualDocumentView::full_text`.
    pub byte_span: (usize, usize),
    /// Inline or block-level `SourceId`s cited by this block.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub cited_sources: Vec<SourceId>,
    /// True for prose paragraphs/quotes that should be evaluated for vacuousness, logic, and grounding.
    pub is_evaluatable_prose: bool,
}

/// Full-document context package with boundary cues for Late Chunking and Contextual Judge models.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
pub struct ContextualDocumentView {
    pub title: String,
    /// Concatenated full-document text across all blocks in display order.
    pub full_text: String,
    /// Ordered per-block spans indexing into `full_text`.
    pub spans: Vec<ContextualBlockSpan>,
    /// Structured evidence facts extracted from tables, ECharts data projections, and bibliography sources.
    pub grounding_facts: Vec<GroundingFact>,
}

/// One atomic reference fact extracted from a table, chart or registered `SourceEntry` for grounding checks.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
pub struct GroundingFact {
    /// Fact identifier (`"block:tbl_1"` or `"source:sc-discipline-eval"`).
    pub fact_id: String,
    pub title: String,
    pub fact_text: String,
}

/// Compact section-scoped evaluation batch to avoid Context Rot and stay well within Jev's 32k per-question limit.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
pub struct SectionEvaluationBatch {
    pub section_key: String,
    pub section_path: Vec<String>,
    /// Document-level heading outline for global context without sending unrelated chapters.
    pub document_outline: Vec<String>,
    /// Target prose blocks belonging to this section.
    pub blocks: Vec<ContextualBlockSpan>,
    /// Sentence-level spans across `blocks`, numbered deterministically (`s_1`, `s_2`, ...).
    pub sentences: Vec<SentenceSpan>,
    /// Filtered `GroundingFact`s relevant to this section (cited sources + section tables/charts).
    pub relevant_facts: Vec<GroundingFact>,
}

/// Quality dimensions scored by contextual evaluation models.
#[derive(
    Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize, JsonSchema,
)]
#[serde(rename_all = "snake_case")]
pub enum EvaluationDimension {
    /// Substantive evidence & analytical density (`1.0` = dense empirical prose, `0.0` = vacuous AI filler).
    InformationDensity,
    /// Internal and cross-paragraph logical coherence (`1.0` = sound reasoning, `0.0` = contradiction/non-sequitur).
    LogicalCoherence,
    /// Numerical and directional consistency with surrounding tables, charts, and paragraphs.
    CrossBlockDataConsistency,
    /// Alignment between cited claims and registered source evidence.
    SourceGroundedness,
    /// Non-redundancy against prior paragraphs (`1.0` = distinct contribution, `0.0` = duplicate restatement).
    CrossBlockRedundancy,
    /// Topical alignment with the enclosing section heading and visual blocks.
    SectionRelevance,
}

impl EvaluationDimension {
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::InformationDensity => "information_density",
            Self::LogicalCoherence => "logical_coherence",
            Self::CrossBlockDataConsistency => "cross_block_data_consistency",
            Self::SourceGroundedness => "source_groundedness",
            Self::CrossBlockRedundancy => "cross_block_redundancy",
            Self::SectionRelevance => "section_relevance",
        }
    }
}

/// Sentence- or claim-level grounding verification result (modeled after Google Cloud CheckGrounding).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct ClaimGroundingScore {
    pub claim_text: String,
    /// Normalized support score in `0.0..=1.0`.
    pub support_score: f32,
    /// Supporting `GroundingFact::fact_id` references.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub supporting_facts: Vec<String>,
    /// Optional explanation when a claim contradicts or lacks support in the document context.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub contradiction_note: Option<String>,
}

/// Confidence-gated escalation status following `arXiv:2609.26550` (*Accept When Confident, Escalate When Unsure*).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum EscalationStatus {
    /// High-confidence clean pass (`q >= tau`, no issue detected).
    AcceptedClean,
    /// High-confidence defect detected (`q >= tau`), ready to surface directly as a review comment thread.
    FlaggedConfident,
    /// Low-confidence verdict (`q < tau`), escalated for reasoning-LLM review before finalizing.
    EscalatedToReasoningLlm,
}

/// Actionable semantic finding anchored to an exact sentence/block, ready for `CommentThread` persistence.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct EvaluationFinding {
    pub block_id: BlockId,
    /// Open rubric/dimension label (e.g. `"source_groundedness"`, `"information_density"`).
    pub dimension: String,
    /// Exact sentence or phrase in the block to anchor `TextQuoteSelector`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub quote_exact: Option<String>,
    /// Human-readable review comment explaining the issue or escalation reason.
    pub summary: String,
    /// Concrete revision guidance.
    pub suggestion: String,
    /// Model confidence `q` (`0.0..=1.0`).
    pub confidence: f32,
    /// Optional normalized score (`0.0..=1.0`).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub score: Option<f32>,
    /// Escalation outcome under the configured confidence gate `tau`.
    pub status: EscalationStatus,
    /// Optional `GroundingFact::fact_id` associated with this finding.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub evidence_fact_id: Option<String>,
}

/// Contextual evaluation output for a single report block.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct BlockEvaluationResult {
    pub block_id: BlockId,
    /// Normalized dimension scores (`0.0..=1.0`).
    pub scores: BTreeMap<EvaluationDimension, f32>,
    /// Highest cross-block cosine similarity detected via Late Chunking embeddings (`(other_block_id, similarity)`).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub most_similar_block: Option<(BlockId, f32)>,
    /// Claim-level grounding results when fact verification was executed.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub claims: Vec<ClaimGroundingScore>,
    /// Actionable findings ready to be surfaced as side-channel `CommentThread`s.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub findings: Vec<EvaluationFinding>,
}

impl BlockEvaluationResult {
    /// Convert low dimension scores, cross-block redundancy, or contradicted claims into
    /// structured `EvaluationFinding` items (without emitting `QR6xx` lint codes).
    pub fn synthesize_findings(&mut self, default_confidence: f32) {
        let first_sentence = |text: &str| -> Option<String> {
            let trimmed = text.trim();
            if trimmed.is_empty() {
                None
            } else {
                Some(trimmed.chars().take(200).collect())
            }
        };

        if let Some(&density) = self.scores.get(&EvaluationDimension::InformationDensity)
            && density < 0.45
        {
            self.findings.push(EvaluationFinding {
                block_id: self.block_id.clone(),
                dimension: EvaluationDimension::InformationDensity.as_str().into(),
                quote_exact: None,
                summary: format!(
                    "段落 `{}` 的上下文信息密度偏低（{density:.2}），存在空泛表述或套话风险。",
                    self.block_id
                ),
                suggestion: "补充具体的对比数据、机制拆解或可核查的实证事实，替换泛化过渡语。"
                    .into(),
                confidence: default_confidence,
                score: Some(density),
                status: EscalationStatus::FlaggedConfident,
                evidence_fact_id: None,
            });
        }

        if let Some(&consistency) = self
            .scores
            .get(&EvaluationDimension::CrossBlockDataConsistency)
            && consistency < 0.50
        {
            let contradicting_claim = self
                .claims
                .iter()
                .find(|claim| claim.contradiction_note.is_some());
            let detail = contradicting_claim
                .and_then(|claim| claim.contradiction_note.as_deref())
                .unwrap_or("正文的数值、排名或变化方向与同文档表格/图表存在不一致。");
            self.findings.push(EvaluationFinding {
                block_id: self.block_id.clone(),
                dimension: EvaluationDimension::CrossBlockDataConsistency
                    .as_str()
                    .into(),
                quote_exact: contradicting_claim.and_then(|c| first_sentence(&c.claim_text)),
                summary: format!("跨块数据一致性异常（{consistency:.2}）：{detail}"),
                suggestion:
                    "核对本段数值、时间窗口和排名与同节表格/ECharts 块及全局事实口径是否一致。"
                        .into(),
                confidence: default_confidence,
                score: Some(consistency),
                status: EscalationStatus::FlaggedConfident,
                evidence_fact_id: contradicting_claim
                    .and_then(|c| c.supporting_facts.first().cloned()),
            });
        }

        if let Some(&grounded) = self.scores.get(&EvaluationDimension::SourceGroundedness)
            && grounded < 0.50
        {
            self.findings.push(EvaluationFinding {
                block_id: self.block_id.clone(),
                dimension: EvaluationDimension::SourceGroundedness.as_str().into(),
                quote_exact: None,
                summary: format!(
                    "段落 `{}` 的关键论断缺乏足够引文或事实支撑（{grounded:.2}）。",
                    self.block_id
                ),
                suggestion:
                    "核查引用的 `[^source_id]` 原始证据是否支撑该结论，或收窄未经证实的因果推论。"
                        .into(),
                confidence: default_confidence,
                score: Some(grounded),
                status: EscalationStatus::FlaggedConfident,
                evidence_fact_id: None,
            });
        }

        if let Some((other_id, sim)) = self.most_similar_block.clone()
            && sim >= 0.92
        {
            self.findings.push(EvaluationFinding {
                block_id: self.block_id.clone(),
                dimension: EvaluationDimension::CrossBlockRedundancy.as_str().into(),
                quote_exact: None,
                summary: format!(
                    "段落 `{}` 与 `{other_id}` 语义高度重复（上下文余弦相似度 {sim:.2}）。",
                    self.block_id
                ),
                suggestion: format!("建议与 `{other_id}` 合并提炼，或在本段聚焦差异化的分析维度。"),
                confidence: default_confidence,
                score: Some(1.0 - sim),
                status: EscalationStatus::FlaggedConfident,
                evidence_fact_id: Some(format!("block:{other_id}")),
            });
        }

        if let Some(&relevance) = self.scores.get(&EvaluationDimension::SectionRelevance)
            && relevance < 0.40
        {
            self.findings.push(EvaluationFinding {
                block_id: self.block_id.clone(),
                dimension: EvaluationDimension::SectionRelevance.as_str().into(),
                quote_exact: None,
                summary: format!(
                    "段落 `{}` 与所在章节标题主题存在偏离（{relevance:.2}）。",
                    self.block_id
                ),
                suggestion: "调整段落核心论点使其呼应当前小节标题，或将该段移至更匹配的章节。"
                    .into(),
                confidence: default_confidence,
                score: Some(relevance),
                status: EscalationStatus::FlaggedConfident,
                evidence_fact_id: None,
            });
        }

        if let Some(&coherence) = self.scores.get(&EvaluationDimension::LogicalCoherence)
            && coherence < 0.50
        {
            self.findings.push(EvaluationFinding {
                block_id: self.block_id.clone(),
                dimension: EvaluationDimension::LogicalCoherence.as_str().into(),
                quote_exact: None,
                summary: format!(
                    "段落 `{}` 存在上下文逻辑断层或因果跳跃（{coherence:.2}）。",
                    self.block_id
                ),
                suggestion: "补全前提条件与因果链条，确保与相邻段落的论证承接自然一致。".into(),
                confidence: default_confidence,
                score: Some(coherence),
                status: EscalationStatus::FlaggedConfident,
                evidence_fact_id: None,
            });
        }
    }
}

/// Segment a prose block into exact sentence substrings for Jev `Choice` anchoring and `TextQuoteSelector`.
pub fn segment_prose_sentences(
    block_id: &BlockId,
    raw_text: &str,
    start_index: usize,
) -> Vec<SentenceSpan> {
    // Strip leading `<!-- block:... -->` projection metadata lines so `TextQuoteSelector::exact`
    // matches visible prose only.
    let cleaned_lines: Vec<&str> = raw_text
        .lines()
        .filter(|line| {
            let t = line.trim();
            !(t.starts_with("<!--") && t.ends_with("-->"))
                && !(t.starts_with("[^") && t.contains("]:"))
        })
        .collect();
    let owned_text = cleaned_lines.join("\n");
    let text = owned_text.as_str();
    let mut sentences = Vec::new();
    let mut chunk_start = 0;
    let mut idx = start_index;

    for (pos, ch) in text.char_indices() {
        let is_boundary = matches!(ch, '。' | '！' | '？' | '；' | '\n')
            || (matches!(ch, '.' | '!' | '?')
                && text[pos + ch.len_utf8()..]
                    .chars()
                    .next()
                    .is_none_or(|next| next.is_whitespace()));
        if is_boundary {
            let end = pos + ch.len_utf8();
            let candidate = text[chunk_start..end].trim();
            if candidate.chars().count() >= 6 {
                sentences.push(SentenceSpan {
                    sentence_id: format!("s_{idx}"),
                    block_id: block_id.clone(),
                    exact_text: candidate.to_string(),
                });
                idx += 1;
            }
            chunk_start = end;
        }
    }

    if chunk_start < text.len() {
        let tail = text[chunk_start..].trim();
        if tail.chars().count() >= 6 {
            sentences.push(SentenceSpan {
                sentence_id: format!("s_{idx}"),
                block_id: block_id.clone(),
                exact_text: tail.to_string(),
            });
        }
    }

    if sentences.is_empty() && !text.trim().is_empty() {
        sentences.push(SentenceSpan {
            sentence_id: format!("s_{idx}"),
            block_id: block_id.clone(),
            exact_text: text.trim().chars().take(1_500).collect(),
        });
    }

    sentences
}

/// Partition a `ContextualDocumentView` into compact section-scoped batches (filtering out unrelated
/// chapters and unrelated facts to prevent Context Rot and stay within Jev's 32k per-question limit).
pub fn partition_section_batches(
    view: &ContextualDocumentView,
    target_blocks: Option<&[BlockId]>,
) -> Vec<SectionEvaluationBatch> {
    let target_filter: Option<BTreeSet<&BlockId>> =
        target_blocks.map(|ids| ids.iter().collect::<BTreeSet<_>>());

    let mut document_outline = Vec::new();
    let mut section_blocks: BTreeMap<String, (Vec<String>, Vec<ContextualBlockSpan>)> =
        BTreeMap::new();
    let mut section_fact_ids: BTreeMap<String, BTreeSet<String>> = BTreeMap::new();
    let mut section_order = Vec::new();

    for span in &view.spans {
        if !span.is_evaluatable_prose && !span.section_path.is_empty() {
            let heading = span.section_path.join(" > ");
            if !document_outline.contains(&heading) {
                document_outline.push(heading);
            }
        }
        let section_key = if span.section_path.is_empty() {
            "__root__".to_string()
        } else {
            span.section_path.join(" > ")
        };

        if !span.is_evaluatable_prose {
            // Register table/chart facts in this section so prose in the same section can check them.
            let candidate_fact_id = format!("block:{}", span.block_id);
            if view
                .grounding_facts
                .iter()
                .any(|f| f.fact_id == candidate_fact_id)
            {
                section_fact_ids
                    .entry(section_key)
                    .or_default()
                    .insert(candidate_fact_id);
            }
            continue;
        }

        if let Some(filter) = &target_filter
            && !filter.contains(&span.block_id)
        {
            continue;
        }

        if !section_blocks.contains_key(&section_key) {
            section_order.push(section_key.clone());
        }
        let fact_set = section_fact_ids.entry(section_key.clone()).or_default();
        for sid in &span.cited_sources {
            fact_set.insert(format!("source:{sid}"));
        }
        section_blocks
            .entry(section_key)
            .or_insert_with(|| (span.section_path.clone(), Vec::new()))
            .1
            .push(span.clone());
    }

    let facts_by_id: BTreeMap<&str, &GroundingFact> = view
        .grounding_facts
        .iter()
        .map(|f| (f.fact_id.as_str(), f))
        .collect();

    let mut batches = Vec::with_capacity(section_order.len());
    for section_key in section_order {
        let Some((section_path, blocks)) = section_blocks.remove(&section_key) else {
            continue;
        };
        let mut sentences = Vec::new();
        let mut next_idx = 1;
        for block in &blocks {
            let prose_src = block.raw_text.as_deref().unwrap_or(&block.rendered_text);
            let mut block_sentences = segment_prose_sentences(&block.block_id, prose_src, next_idx);
            next_idx += block_sentences.len();
            sentences.append(&mut block_sentences);
        }

        let relevant_facts = section_fact_ids
            .get(&section_key)
            .map(|ids| {
                ids.iter()
                    .filter_map(|id| facts_by_id.get(id.as_str()).map(|f| (*f).clone()))
                    .collect::<Vec<_>>()
            })
            .unwrap_or_default();

        batches.push(SectionEvaluationBatch {
            section_key,
            section_path,
            document_outline: document_outline.clone(),
            blocks,
            sentences,
            relevant_facts,
        });
    }

    batches
}

/// Build a `ContextualDocumentView` from a `ReportDocument`, preserving exact byte spans for
/// Late Chunking and extracting structured facts from tables, charts, and bibliography entries.
pub fn build_contextual_document_view(document: &ReportDocument) -> ContextualDocumentView {
    let citation_scan = scan_document_citations(document);
    let mut cites_by_block: BTreeMap<BlockId, Vec<SourceId>> = BTreeMap::new();
    for usage in citation_scan.uses {
        let entry = cites_by_block.entry(usage.block_id).or_default();
        if !entry.contains(&usage.source_id) {
            entry.push(usage.source_id);
        }
    }

    let mut full_text = String::new();
    let mut spans = Vec::with_capacity(document.blocks.len());
    let mut heading_stack: Vec<(u8, String)> = Vec::new();
    let mut grounding_facts = Vec::new();
    let md_options = MarkdownOptions {
        include_chart_data: true,
        include_front_matter: false,
    };

    for block in &document.blocks {
        if let BlockKind::Heading { level, text } = &block.body {
            heading_stack.retain(|(lvl, _)| *lvl < *level);
            heading_stack.push((*level, text.trim().to_string()));
        }
        let section_path: Vec<String> = heading_stack
            .iter()
            .map(|(_, title)| title.clone())
            .collect();

        let rendered = project_blocks(document, std::slice::from_ref(&block.id), &md_options)
            .unwrap_or_else(|_| match &block.body {
                BlockKind::Heading { text, .. }
                | BlockKind::Paragraph { text }
                | BlockKind::Quote { text, .. } => text.clone(),
                BlockKind::Table(table) => table.title.clone().unwrap_or_default(),
                BlockKind::Echarts(chart) => chart.title.clone().unwrap_or_default(),
                BlockKind::CustomHtml(html) => html.fallback_text.clone(),
            })
            .trim()
            .to_string();

        if !full_text.is_empty() && !rendered.is_empty() {
            full_text.push_str("\n\n");
        }
        let start = full_text.len();
        full_text.push_str(&rendered);
        let end = full_text.len();

        let raw_text = match &block.body {
            BlockKind::Paragraph { text } | BlockKind::Quote { text, .. } => Some(text.clone()),
            _ => None,
        };
        let is_evaluatable_prose = raw_text.is_some();

        match &block.body {
            BlockKind::Table(table) => {
                grounding_facts.push(GroundingFact {
                    fact_id: format!("block:{}", block.id),
                    title: table
                        .title
                        .clone()
                        .unwrap_or_else(|| format!("Table {}", block.id)),
                    fact_text: rendered.clone(),
                });
            }
            BlockKind::Echarts(chart) => {
                grounding_facts.push(GroundingFact {
                    fact_id: format!("block:{}", block.id),
                    title: chart
                        .title
                        .clone()
                        .unwrap_or_else(|| format!("Chart {}", block.id)),
                    fact_text: rendered.clone(),
                });
            }
            _ => {}
        }

        let mut cited_sources = cites_by_block.remove(&block.id).unwrap_or_default();
        for sid in &block.source_ids {
            if !cited_sources.contains(sid) {
                cited_sources.push(sid.clone());
            }
        }

        spans.push(ContextualBlockSpan {
            block_id: block.id.clone(),
            section_path,
            rendered_text: rendered,
            raw_text,
            byte_span: (start, end),
            cited_sources,
            is_evaluatable_prose,
        });
    }

    for (source_id, source) in &document.sources {
        let mut details = vec![source.title.clone()];
        if let Some(publisher) = &source.publisher {
            details.push(format!("publisher: {publisher}"));
        }
        if let Some(date) = &source.date {
            details.push(format!("date: {date}"));
        }
        if let Some(note) = &source.note {
            details.push(format!("scope: {note}"));
        }
        if let Some(excerpt) = &source.excerpt {
            details.push(format!("excerpt: {excerpt}"));
        }
        grounding_facts.push(GroundingFact {
            fact_id: format!("source:{source_id}"),
            title: source.title.clone(),
            fact_text: details.join(" | "),
        });
    }

    ContextualDocumentView {
        title: document.title(),
        full_text,
        spans,
        grounding_facts,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{
        Block,
        block::{CellValue, ColumnNode, TableBlock},
        ids::DocId,
        model::SourceEntry,
    };

    #[test]
    fn contextual_view_preserves_exact_byte_spans_and_extracts_facts() {
        let mut doc = ReportDocument::new(DocId::parse("doc_eval").expect("doc id"));
        let sc_eval_id = SourceId::parse("sc-eval").expect("source id");
        doc.sources.insert(
            sc_eval_id.clone(),
            SourceEntry {
                title: "第五轮学科评估统计".into(),
                url: None,
                authors: Vec::new(),
                publisher: Some("教育部".into()),
                date: Some("2025".into()),
                accessed: None,
                note: Some("公开档位统计口径".into()),
                excerpt: None,
            },
        );
        doc.blocks.push(Block::new(
            BlockId::parse("h1").expect("block id"),
            BlockKind::Heading {
                level: 1,
                text: "学科建设：高峰学科集中度显著提升".into(),
            },
        ));
        let mut p1 = Block::new(
            BlockId::parse("p1").expect("block id"),
            BlockKind::Paragraph {
                text: "浙江大学在高峰学科建设中保持领先[^sc-eval]。与此同时，区域协同进一步增强。"
                    .into(),
            },
        );
        p1.source_ids.push(sc_eval_id.clone());
        doc.blocks.push(p1);
        let mut row = BTreeMap::new();
        row.insert("school".into(), CellValue::text("浙江大学"));
        doc.blocks.push(Block::new(
            BlockId::parse("tbl1").expect("block id"),
            BlockKind::Table(TableBlock {
                title: Some("核心学科对比".into()),
                columns: vec![ColumnNode::leaf("school", "高校")],
                rows: vec![row],
                summary_rows: Vec::new(),
                note: None,
                numbering: None,
            }),
        ));

        let view = build_contextual_document_view(&doc);
        assert_eq!(view.spans.len(), 3);
        for span in &view.spans {
            assert_eq!(
                &view.full_text[span.byte_span.0..span.byte_span.1],
                span.rendered_text.as_str()
            );
        }
        assert!(!view.spans[0].is_evaluatable_prose);
        assert!(view.spans[1].is_evaluatable_prose);
        assert_eq!(
            view.spans[1].section_path,
            vec!["学科建设：高峰学科集中度显著提升"]
        );
        assert_eq!(view.spans[1].cited_sources, vec![sc_eval_id]);
        assert_eq!(view.grounding_facts.len(), 2);

        let batches = partition_section_batches(&view, None);
        assert_eq!(batches.len(), 1);
        assert_eq!(batches[0].sentences.len(), 2);
        assert_eq!(batches[0].relevant_facts.len(), 2);
    }

    #[test]
    fn synthesize_findings_emits_structured_comment_findings_without_qr_codes() {
        let mut scores = BTreeMap::new();
        scores.insert(EvaluationDimension::InformationDensity, 0.30);
        scores.insert(EvaluationDimension::CrossBlockDataConsistency, 0.25);
        let mut result = BlockEvaluationResult {
            block_id: BlockId::parse("p_bad").expect("block id"),
            scores,
            most_similar_block: Some((BlockId::parse("p_prev").expect("block id"), 0.95)),
            claims: vec![ClaimGroundingScore {
                claim_text: "A校经费12亿".into(),
                support_score: 0.1,
                supporting_facts: vec!["block:tbl_1".into()],
                contradiction_note: Some("Table tbl_1 states 15.2亿".into()),
            }],
            findings: Vec::new(),
        };
        result.synthesize_findings(0.92);
        let dims: Vec<&str> = result
            .findings
            .iter()
            .map(|f| f.dimension.as_str())
            .collect();
        assert_eq!(
            dims,
            vec![
                "information_density",
                "cross_block_data_consistency",
                "cross_block_redundancy"
            ]
        );
        assert_eq!(
            result.findings[1].quote_exact.as_deref(),
            Some("A校经费12亿")
        );
        assert_eq!(
            result.findings[1].evidence_fact_id.as_deref(),
            Some("block:tbl_1")
        );
    }
}
