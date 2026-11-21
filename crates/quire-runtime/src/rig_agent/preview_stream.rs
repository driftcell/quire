//! Streamed `apply_operations` argument previews and anchor inference.

use std::{
    collections::{BTreeSet, HashMap},
    time::{Duration, Instant},
};

use quire_protocol::{
    BlockId, EphemeralPayload, EphemeralPreviewEvent, PartialBlockPreview, PreviewAnchor,
    PreviewStatus,
};
use rig_core::{
    message::{ToolCall, ToolName},
    streaming::{PartKind, parse_partial_arguments},
};
use secrecy::{ExposeSecret, SecretString};
use serde_json::{Value, json};

use crate::{
    config::Config,
    error::{RuntimeError, RuntimeResult},
};

use super::AgentExecution;

const PREVIEW_TOOLS: &[&str] = &["apply_operations"];
const PREVIEW_ANCHOR_SETTLE: Duration = Duration::from_millis(200);

/// Throttle for streamed tool-argument preview frames.
#[derive(Debug)]
pub struct PreviewThrottle {
    interval: Duration,
    last_emit: Option<Instant>,
}

impl PreviewThrottle {
    pub fn new(milliseconds: u64) -> Self {
        Self {
            interval: Duration::from_millis(milliseconds.clamp(50, 80)),
            last_emit: None,
        }
    }

    pub fn should_emit(&mut self) -> bool {
        self.should_emit_at(Instant::now())
    }

    fn should_emit_at(&mut self, now: Instant) -> bool {
        if self
            .last_emit
            .is_some_and(|last| now.duration_since(last) < self.interval)
        {
            return false;
        }
        self.last_emit = Some(now);
        true
    }
}

/// Sends partial tool-argument previews to Quire's ephemeral preview endpoint.
pub struct ToolPreviewStreamer {
    http: reqwest::Client,
    base_url: String,
    token: SecretString,
    doc_id: String,
    preview_id: String,
    actor: String,
    block_id: String,
    anchor: PreviewAnchor,
    seq: u64,
    throttle: PreviewThrottle,
}

impl ToolPreviewStreamer {
    pub fn new(
        base_url: impl Into<String>,
        token: SecretString,
        doc_id: impl Into<String>,
        preview_id: impl Into<String>,
        actor: impl Into<String>,
        block_id: impl Into<String>,
        throttle_ms: u64,
    ) -> Self {
        let block_id = block_id.into();
        Self {
            http: crate::http_client::timed_client(Duration::from_secs(30)),
            base_url: base_url.into().trim_end_matches('/').to_string(),
            token,
            doc_id: doc_id.into(),
            preview_id: preview_id.into(),
            actor: actor.into(),
            anchor: PreviewAnchor::ReplaceBlock {
                block_id: quire_protocol::BlockId::pending(),
            },
            block_id,
            seq: 0,
            throttle: PreviewThrottle::new(throttle_ms),
        }
    }

    /// Override where the previewed block appears. Replace anchors are rewritten to the
    /// streamer's block ID; AfterBlock and BeforeBlock targets are preserved.
    pub fn with_anchor(mut self, anchor: PreviewAnchor) -> Self {
        self.anchor = anchor;
        self
    }

    pub fn set_anchor(&mut self, anchor: PreviewAnchor) {
        self.anchor = anchor;
    }

    pub fn preview_id(&self) -> &str {
        &self.preview_id
    }

    pub async fn push_delta(&mut self, text: &str) -> RuntimeResult<bool> {
        let block_id = quire_protocol::BlockId::parse(self.block_id.clone())
            .map_err(|error| RuntimeError::InvalidRequest(error.to_string()))?;
        self.push_block(PartialBlockPreview {
            block_id: Some(block_id),
            block_type: "paragraph".into(),
            title: None,
            level: None,
            text: Some(text.to_string()),
        })
        .await
    }

    /// Build the next ephemeral event with the resolved anchor without sending it.
    fn next_preview_event(
        &mut self,
        block: PartialBlockPreview,
    ) -> RuntimeResult<EphemeralPreviewEvent> {
        self.seq = self.seq.saturating_add(1);
        let block_id = quire_protocol::BlockId::parse(self.block_id.clone())
            .map_err(|error| RuntimeError::InvalidRequest(error.to_string()))?;
        let anchor = match self.anchor.clone() {
            PreviewAnchor::ReplaceBlock { .. } => PreviewAnchor::ReplaceBlock {
                block_id: block_id.clone(),
            },
            other => other,
        };
        Ok(EphemeralPreviewEvent {
            doc_id: quire_protocol::DocId::parse(self.doc_id.clone())
                .map_err(|error| RuntimeError::InvalidRequest(error.to_string()))?,
            preview_id: self.preview_id.clone(),
            actor: self.actor.clone(),
            anchor,
            status: PreviewStatus::Streaming,
            seq: Some(self.seq),
            ttl_ms: None,
            block,
            received_at: chrono::Utc::now(),
        })
    }

    /// Push one partial block frame. The streamer remains responsible for throttling and
    /// monotonic sequence numbers.
    pub async fn push_block(&mut self, block: PartialBlockPreview) -> RuntimeResult<bool> {
        if !self.throttle.should_emit() {
            return Ok(false);
        }
        self.send_block(block).await
    }

    /// Push the final frame even when the throttle window has not elapsed. Used once the
    /// complete tool arguments reveal the real anchor so an earlier provisional frame
    /// cannot be the last event the frontend sees.
    pub(super) async fn push_block_now(
        &mut self,
        block: PartialBlockPreview,
    ) -> RuntimeResult<bool> {
        self.send_block(block).await
    }

    async fn send_block(&mut self, block: PartialBlockPreview) -> RuntimeResult<bool> {
        let event = self.next_preview_event(block)?;
        let payload = EphemeralPayload::Preview { event };
        let response = self
            .http
            .post(format!(
                "{}/api/docs/{}/ephemeral",
                self.base_url, self.doc_id
            ))
            .header(
                reqwest::header::AUTHORIZATION,
                format!("Bearer {}", self.token.expose_secret()),
            )
            .json(&payload)
            .send()
            .await?;
        if !response.status().is_success() {
            return Err(RuntimeError::Internal(format!(
                "preview publish failed with {}",
                response.status()
            )));
        }
        Ok(true)
    }
}

#[derive(Clone)]
struct PreviewBlock {
    block_id: Option<String>,
    block_type: String,
    title: Option<String>,
    level: Option<u8>,
    text: Option<String>,
    anchor: PreviewAnchor,
}

impl PreviewBlock {
    fn partial(&self, block_id: BlockId) -> PartialBlockPreview {
        PartialBlockPreview {
            block_id: Some(block_id),
            block_type: self.block_type.clone(),
            title: self.title.clone(),
            level: self.level,
            text: self.text.clone(),
        }
    }
}

struct StreamingPreview {
    tool_name: String,
    buffer: String,
    streamers: HashMap<String, ToolPreviewStreamer>,
    preview_ids: BTreeSet<String>,
}

pub(super) struct PreviewState {
    run_id: String,
    doc_id: Option<String>,
    base_url: String,
    token: SecretString,
    actor: String,
    throttle_ms: u64,
    streams: HashMap<usize, StreamingPreview>,
    previews_by_call: HashMap<String, Vec<String>>,
}

impl PreviewState {
    pub(super) fn new(execution: &AgentExecution, config: &Config) -> Self {
        Self {
            run_id: execution.run_id.clone(),
            doc_id: execution.doc_id.clone(),
            base_url: config.quire_base_url.clone(),
            token: config.quire_rw_token.clone(),
            actor: execution.agent_id.clone(),
            throttle_ms: config.preview_throttle_ms,
            streams: HashMap::new(),
            previews_by_call: HashMap::new(),
        }
    }

    pub(super) fn on_start(&mut self, part: usize, kind: PartKind, name: Option<&ToolName>) {
        if kind != PartKind::ToolCall {
            return;
        }
        let Some(name) = name else {
            return;
        };
        if !PREVIEW_TOOLS.contains(&name.as_str()) || self.doc_id.is_none() {
            return;
        }
        self.streams.insert(
            part,
            StreamingPreview {
                tool_name: name.as_str().to_string(),
                buffer: String::new(),
                streamers: HashMap::new(),
                preview_ids: BTreeSet::new(),
            },
        );
    }

    pub(super) async fn on_arguments(&mut self, part: usize, fragment: &str) {
        let Some(stream) = self.streams.get_mut(&part) else {
            return;
        };
        stream.buffer.push_str(fragment);
        let Some(doc_id) = self.doc_id.clone() else {
            return;
        };
        let arguments = Value::Object(parse_partial_arguments(&stream.buffer));
        let blocks = extract_preview_blocks(&arguments);
        for (index, candidate) in blocks.iter().enumerate() {
            let key = candidate
                .block_id
                .clone()
                .unwrap_or_else(|| format!("auto-{index}"));
            if !stream.streamers.contains_key(&key) {
                let block_id = candidate
                    .block_id
                    .clone()
                    .unwrap_or_else(|| format!("preview{part}{index}"));
                let preview_id = format!("{}:{}:{}:{}", self.run_id, stream.tool_name, part, key);
                stream.streamers.insert(
                    key.clone(),
                    ToolPreviewStreamer::new(
                        &self.base_url,
                        self.token.clone(),
                        &doc_id,
                        &preview_id,
                        &self.actor,
                        &block_id,
                        self.throttle_ms,
                    )
                    .with_anchor(candidate.anchor.clone()),
                );
            }
            if let Some(streamer) = stream.streamers.get_mut(&key) {
                if candidate.anchor != PreviewAnchor::AppendToDoc {
                    streamer.set_anchor(candidate.anchor.clone());
                }
                let block_id = BlockId::parse(
                    candidate
                        .block_id
                        .clone()
                        .unwrap_or_else(|| format!("preview{part}{index}")),
                )
                .unwrap_or_else(|_| BlockId::pending());
                if let Err(error) = streamer.push_block(candidate.partial(block_id)).await {
                    tracing::warn!(%error, tool = %stream.tool_name, "ephemeral preview publish failed");
                } else {
                    stream.preview_ids.insert(preview_id_for(streamer));
                }
            }
        }
    }

    pub(super) async fn on_end(&mut self, part: usize, call: &ToolCall) {
        let Some(mut stream) = self.streams.remove(&part) else {
            return;
        };
        let Some(doc_id) = self.doc_id.clone() else {
            return;
        };
        // The model may emit `blocks` before `target_id`/`section_id`/`type`. Frames sent
        // while those fields are missing carry the provisional `AppendToDoc` anchor. Once
        // the complete arguments arrive, republish every block with its resolved anchor and
        // give the frontend one paint window before the commit clears the previews.
        let mut corrected = false;
        let final_blocks = extract_preview_blocks(&call.function.arguments_value());
        for (index, candidate) in final_blocks.iter().enumerate() {
            let key = candidate
                .block_id
                .clone()
                .unwrap_or_else(|| format!("auto-{index}"));
            if !stream.streamers.contains_key(&key) {
                let block_id = candidate
                    .block_id
                    .clone()
                    .unwrap_or_else(|| format!("preview{part}{index}"));
                let preview_id = format!("{}:{}:{}:{}", self.run_id, stream.tool_name, part, key);
                stream.streamers.insert(
                    key.clone(),
                    ToolPreviewStreamer::new(
                        &self.base_url,
                        self.token.clone(),
                        &doc_id,
                        &preview_id,
                        &self.actor,
                        &block_id,
                        self.throttle_ms,
                    )
                    .with_anchor(candidate.anchor.clone()),
                );
                corrected = true;
            }
            let Some(streamer) = stream.streamers.get_mut(&key) else {
                continue;
            };
            if streamer.anchor != candidate.anchor {
                streamer.set_anchor(candidate.anchor.clone());
                corrected = true;
            }
            let block_id = BlockId::parse(
                candidate
                    .block_id
                    .clone()
                    .unwrap_or_else(|| format!("preview{part}{index}")),
            )
            .unwrap_or_else(|_| BlockId::pending());
            match streamer.push_block_now(candidate.partial(block_id)).await {
                Ok(_) => {
                    stream.preview_ids.insert(preview_id_for(streamer));
                }
                Err(error) => {
                    tracing::warn!(%error, tool = %stream.tool_name, "ephemeral preview publish failed");
                }
            }
        }
        if corrected {
            tokio::time::sleep(PREVIEW_ANCHOR_SETTLE).await;
        }
        if !stream.preview_ids.is_empty() {
            self.previews_by_call.insert(
                call.id.wire().to_string(),
                stream.preview_ids.into_iter().collect(),
            );
        }
    }

    pub(super) fn take_preview_ids(&mut self, call_id: &str) -> Option<Vec<String>> {
        self.previews_by_call.remove(call_id)
    }

    #[cfg(test)]
    pub(super) fn for_test(run_id: &str) -> Self {
        Self {
            run_id: run_id.to_string(),
            doc_id: None,
            base_url: String::new(),
            token: SecretString::from(String::new()),
            actor: "test".into(),
            throttle_ms: 0,
            streams: HashMap::new(),
            previews_by_call: HashMap::new(),
        }
    }
}

fn preview_id_for(streamer: &ToolPreviewStreamer) -> String {
    streamer.preview_id().to_string()
}

fn extract_preview_blocks(arguments: &Value) -> Vec<PreviewBlock> {
    let mut blocks = Vec::new();
    let mut seen = BTreeSet::new();
    collect_preview_blocks(
        arguments,
        0,
        &PreviewAnchor::AppendToDoc,
        &mut blocks,
        &mut seen,
    );
    blocks
}

fn parse_preview_block_id(raw: Option<&str>) -> Option<BlockId> {
    raw.map(str::trim)
        .filter(|raw| !raw.is_empty())
        .and_then(|raw| BlockId::parse(raw.to_string()).ok())
}

fn operation_type(object: &serde_json::Map<String, Value>) -> Option<&str> {
    object.get("type").and_then(Value::as_str)
}

fn is_patch_operation(op_type: Option<&str>, object: &serde_json::Map<String, Value>) -> bool {
    op_type == Some("patch_block")
        || (op_type.is_none() && object.contains_key("patch") && object.contains_key("block_id"))
}

fn infer_operation_anchor(
    object: &serde_json::Map<String, Value>,
    inherited: &PreviewAnchor,
) -> PreviewAnchor {
    let op_type = operation_type(object);
    if op_type == Some("append_to_section") || object.contains_key("section_id") {
        if let Some(section_id) =
            parse_preview_block_id(object.get("section_id").and_then(Value::as_str))
        {
            return PreviewAnchor::AppendToSection { section_id };
        }
        return inherited.clone();
    }
    if op_type == Some("insert_after") {
        if let Some(block_id) =
            parse_preview_block_id(object.get("target_id").and_then(Value::as_str))
        {
            return PreviewAnchor::AfterBlock { block_id };
        }
        return inherited.clone();
    }
    if op_type == Some("insert_before") {
        if let Some(block_id) =
            parse_preview_block_id(object.get("target_id").and_then(Value::as_str))
        {
            return PreviewAnchor::BeforeBlock { block_id };
        }
        return inherited.clone();
    }
    if is_patch_operation(op_type, object) {
        if let Some(block_id) =
            parse_preview_block_id(object.get("block_id").and_then(Value::as_str))
        {
            return PreviewAnchor::ReplaceBlock { block_id };
        }
        return inherited.clone();
    }
    if op_type == Some("replace_range") {
        if let Some(block_id) =
            parse_preview_block_id(object.get("start_id").and_then(Value::as_str))
        {
            return PreviewAnchor::ReplaceBlock { block_id };
        }
        return inherited.clone();
    }
    inherited.clone()
}

fn collect_preview_blocks(
    value: &Value,
    depth: usize,
    anchor: &PreviewAnchor,
    blocks: &mut Vec<PreviewBlock>,
    seen: &mut BTreeSet<String>,
) {
    if depth > 8 {
        return;
    }
    match value {
        Value::Object(object) => {
            let op_type = operation_type(object);
            let next_anchor = infer_operation_anchor(object, anchor);
            let patch_op = is_patch_operation(op_type, object);
            let mut consumed_patch = false;
            if patch_op && let Some(Value::Object(patch)) = object.get("patch") {
                let mut merged = patch.clone();
                if !merged.contains_key("id")
                    && let Some(block_id) = object.get("block_id")
                {
                    merged.insert("id".to_string(), block_id.clone());
                }
                if let Some(block) = preview_block(&merged, &next_anchor) {
                    push_preview_block(block, blocks, seen);
                }
                consumed_patch = true;
            }
            if let Some(block) = preview_block(object, &next_anchor) {
                push_preview_block(block, blocks, seen);
            }
            let chained_insert = matches!(op_type, Some("insert_after" | "insert_before"));
            for (key, child) in object {
                if key == "patch" && consumed_patch {
                    continue;
                }
                if chained_insert && key == "blocks" {
                    collect_chained_insert_blocks(child, depth + 1, &next_anchor, blocks, seen);
                    continue;
                }
                collect_preview_blocks(child, depth + 1, &next_anchor, blocks, seen);
            }
        }
        Value::Array(values) => {
            for child in values {
                collect_preview_blocks(child, depth + 1, anchor, blocks, seen);
            }
        }
        _ => {}
    }
}

fn collect_chained_insert_blocks(
    value: &Value,
    depth: usize,
    anchor: &PreviewAnchor,
    blocks: &mut Vec<PreviewBlock>,
    seen: &mut BTreeSet<String>,
) {
    if depth > 8 {
        return;
    }
    let Value::Array(values) = value else {
        collect_preview_blocks(value, depth, anchor, blocks, seen);
        return;
    };
    let mut chain_anchor = anchor.clone();
    for child in values {
        collect_preview_blocks(child, depth + 1, &chain_anchor, blocks, seen);
        let previous_id = child
            .as_object()
            .and_then(|object| object.get("id"))
            .and_then(Value::as_str);
        if let Some(block_id) = parse_preview_block_id(previous_id) {
            chain_anchor = PreviewAnchor::AfterBlock { block_id };
        }
    }
}

fn push_preview_block(
    block: PreviewBlock,
    blocks: &mut Vec<PreviewBlock>,
    seen: &mut BTreeSet<String>,
) {
    let key = block
        .block_id
        .clone()
        .unwrap_or_else(|| format!("{}:{}", block.block_type, blocks.len()));
    if seen.insert(key) {
        blocks.push(block);
    }
}

fn preview_block(
    object: &serde_json::Map<String, Value>,
    anchor: &PreviewAnchor,
) -> Option<PreviewBlock> {
    let text = object.get("text").and_then(Value::as_str);
    let title = object.get("title").and_then(Value::as_str);
    if text.is_none() && title.is_none() {
        return None;
    }
    let inferred_type = if object.contains_key("level") {
        "heading"
    } else if object.contains_key("rows") || object.contains_key("columns") {
        "table"
    } else if object.contains_key("option") {
        "echarts"
    } else {
        "paragraph"
    };
    let block_type = object
        .get("type")
        .and_then(Value::as_str)
        .unwrap_or(inferred_type);
    if !matches!(
        block_type,
        "heading" | "paragraph" | "quote" | "table" | "echarts" | "custom_html"
    ) {
        return None;
    }
    let block_id = object
        .get("id")
        .and_then(Value::as_str)
        .filter(|value| BlockId::parse((*value).to_string()).is_ok())
        .map(str::to_string);
    let level = object
        .get("level")
        .and_then(Value::as_u64)
        .and_then(|value| u8::try_from(value).ok());
    Some(PreviewBlock {
        block_id,
        block_type: block_type.to_string(),
        title: title.map(str::to_string),
        level,
        text: text.map(str::to_string),
        anchor: anchor.clone(),
    })
}

pub(super) fn inject_preview_ids(arguments: &mut Value, preview_ids: &[String]) {
    if preview_ids.is_empty() {
        return;
    }
    let Some(object) = arguments.as_object_mut() else {
        return;
    };
    let mut merged: Vec<String> = object
        .get("preview_ids")
        .and_then(Value::as_array)
        .map(|values| {
            values
                .iter()
                .filter_map(Value::as_str)
                .map(str::to_string)
                .collect()
        })
        .unwrap_or_default();
    for preview_id in preview_ids {
        if !merged.iter().any(|existing| existing == preview_id) {
            merged.push(preview_id.clone());
        }
    }
    object.insert("preview_ids".to_string(), json!(merged));
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn preview_throttle_respects_the_interval() {
        let mut throttle = PreviewThrottle::new(60);
        let start = Instant::now();
        assert!(throttle.should_emit_at(start));
        assert!(!throttle.should_emit_at(start + Duration::from_millis(20)));
        assert!(throttle.should_emit_at(start + Duration::from_millis(61)));
    }

    #[test]
    fn partial_apply_operations_arguments_produce_block_previews() {
        let raw = r#"{"ops":[{"type":"append","blocks":[{"id":"q3-revenue","type":"paragraph","text":"Revenue grew"#;
        let arguments = Value::Object(parse_partial_arguments(raw));
        let blocks = extract_preview_blocks(&arguments);
        assert_eq!(blocks.len(), 1);
        let block = blocks.first().expect("preview block");
        assert_eq!(block.block_id.as_deref(), Some("q3-revenue"));
        assert_eq!(block.block_type, "paragraph");
        assert_eq!(block.text.as_deref(), Some("Revenue grew"));
    }

    #[test]
    fn preview_anchors_chain_inserted_blocks_and_patch_targets() {
        let arguments = json!({
            "ops": [
                {
                    "type": "insert_after",
                    "target_id": "sec3_h1",
                    "blocks": [
                        {"id": "sec3_p1", "type": "paragraph", "text": "hello"},
                        {"id": "sec3_p2", "type": "paragraph", "text": "world"}
                    ]
                },
                {
                    "block_id": "sec2_p1",
                    "patch": {"text": "updated"},
                    "type": "patch_block"
                }
            ]
        });
        let blocks = extract_preview_blocks(&arguments);
        assert_eq!(blocks.len(), 3);
        assert_eq!(blocks[0].block_id.as_deref(), Some("sec3_p1"));
        assert_eq!(
            blocks[0].anchor,
            PreviewAnchor::AfterBlock {
                block_id: BlockId::parse("sec3_h1").expect("valid target")
            }
        );
        assert_eq!(blocks[1].block_id.as_deref(), Some("sec3_p2"));
        assert_eq!(
            blocks[1].anchor,
            PreviewAnchor::AfterBlock {
                block_id: BlockId::parse("sec3_p1").expect("valid previous block")
            }
        );
        assert_eq!(blocks[2].block_id.as_deref(), Some("sec2_p1"));
        assert_eq!(
            blocks[2].anchor,
            PreviewAnchor::ReplaceBlock {
                block_id: BlockId::parse("sec2_p1").expect("valid patch target")
            }
        );
        assert_eq!(blocks[2].text.as_deref(), Some("updated"));
    }

    #[test]
    fn preview_anchor_chains_insert_before_blocks_in_forward_order() {
        let arguments = json!({
            "ops": [{
                "type": "insert_before",
                "target_id": "sec3_p3",
                "blocks": [
                    {"id": "sec3_p1", "type": "paragraph", "text": "first"},
                    {"id": "sec3_p2", "type": "paragraph", "text": "second"}
                ]
            }]
        });
        let blocks = extract_preview_blocks(&arguments);
        assert_eq!(blocks.len(), 2);
        assert_eq!(
            blocks[0].anchor,
            PreviewAnchor::BeforeBlock {
                block_id: BlockId::parse("sec3_p3").expect("valid target")
            }
        );
        assert_eq!(
            blocks[1].anchor,
            PreviewAnchor::AfterBlock {
                block_id: BlockId::parse("sec3_p1").expect("valid previous block")
            }
        );
    }

    #[test]
    fn partial_patch_operation_anchors_from_block_id_before_type_arrives() {
        let arguments = json!({
            "ops": [{
                "block_id": "ch2_p2",
                "patch": {"text": "updated"}
            }]
        });
        let blocks = extract_preview_blocks(&arguments);
        assert_eq!(blocks.len(), 1);
        let block = blocks.first().expect("partial patch preview");
        assert_eq!(block.block_id.as_deref(), Some("ch2_p2"));
        assert_eq!(block.block_type, "paragraph");
        assert_eq!(block.text.as_deref(), Some("updated"));
        assert_eq!(
            block.anchor,
            PreviewAnchor::ReplaceBlock {
                block_id: BlockId::parse("ch2_p2").expect("patch target")
            }
        );
    }

    #[test]
    fn preview_anchors_follow_real_type_discriminator_wire_format() {
        let arguments = json!({
            "doc_id": "report",
            "ops": [
                {
                    "blocks": [
                        {"id": "ch2_sec", "level": 2, "text": "（四）配套建议", "type": "heading"},
                        {"id": "ch2_p1", "text": "人才引育", "type": "paragraph"}
                    ],
                    "target_id": "ch2_p_update",
                    "type": "insert_after"
                },
                {
                    "block_id": "ch2_p2",
                    "patch": {"text": "updated"},
                    "type": "patch_block"
                }
            ]
        });
        let blocks = extract_preview_blocks(&arguments);
        assert_eq!(blocks.len(), 3);
        assert_eq!(
            blocks[0].anchor,
            PreviewAnchor::AfterBlock {
                block_id: BlockId::parse("ch2_p_update").expect("real target")
            }
        );
        assert_eq!(
            blocks[1].anchor,
            PreviewAnchor::AfterBlock {
                block_id: BlockId::parse("ch2_sec").expect("chained previous block")
            }
        );
        assert_eq!(
            blocks[2].anchor,
            PreviewAnchor::ReplaceBlock {
                block_id: BlockId::parse("ch2_p2").expect("patch target")
            }
        );
        assert_eq!(blocks[2].block_type, "paragraph");
        assert_eq!(blocks[2].text.as_deref(), Some("updated"));
    }

    #[test]
    fn preview_anchor_preserves_after_before_and_replace_targets()
    -> Result<(), Box<dyn std::error::Error>> {
        let target = BlockId::parse("sec2_h1")?;
        let current = BlockId::parse("sec2_p1")?;
        let block = PartialBlockPreview {
            block_id: Some(current.clone()),
            block_type: "paragraph".into(),
            title: None,
            level: None,
            text: Some("streaming".into()),
        };
        let make_streamer = |preview_id: &str, anchor: PreviewAnchor| {
            ToolPreviewStreamer::new(
                "http://127.0.0.1:1",
                SecretString::from("token"),
                "doc",
                preview_id,
                "actor",
                "sec2_p1",
                0,
            )
            .with_anchor(anchor)
        };

        let mut after = make_streamer(
            "preview-after",
            PreviewAnchor::AfterBlock {
                block_id: target.clone(),
            },
        );
        let event = after.next_preview_event(block.clone())?;
        assert_eq!(
            event.anchor,
            PreviewAnchor::AfterBlock {
                block_id: target.clone()
            }
        );

        let mut before = make_streamer(
            "preview-before",
            PreviewAnchor::BeforeBlock {
                block_id: target.clone(),
            },
        );
        let event = before.next_preview_event(block.clone())?;
        assert_eq!(
            event.anchor,
            PreviewAnchor::BeforeBlock {
                block_id: target.clone()
            }
        );

        let mut replace = make_streamer(
            "preview-replace",
            PreviewAnchor::ReplaceBlock { block_id: target },
        );
        let event = replace.next_preview_event(block)?;
        assert_eq!(
            event.anchor,
            PreviewAnchor::ReplaceBlock { block_id: current }
        );
        Ok(())
    }

    #[test]
    fn preview_ids_are_merged_into_tool_arguments() {
        let mut arguments = json!({"doc_id": "report", "preview_ids": ["old"]});
        inject_preview_ids(&mut arguments, &["new".into(), "old".into()]);
        let ids = arguments
            .get("preview_ids")
            .and_then(Value::as_array)
            .expect("preview ids");
        assert_eq!(ids.len(), 2);
        assert_eq!(ids.first().and_then(Value::as_str), Some("old"));
        assert_eq!(ids.last().and_then(Value::as_str), Some("new"));
    }
}
