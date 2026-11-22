//! Streamed `apply_operations` argument previews and anchor inference.

use std::{
    collections::{BTreeSet, HashMap},
    sync::{
        Arc,
        atomic::{AtomicU64, Ordering},
    },
    time::{Duration, Instant},
};

use tokio::sync::watch;

use quire_protocol::{
    BlockId, DocId, EphemeralClearEvent, EphemeralPayload, EphemeralPreviewEvent,
    PartialBlockPreview, PreviewAnchor, PreviewStatus, short_id,
};
use rig::streaming::parse_partial_arguments;
#[cfg(test)]
use rig::{message::ToolName, streaming::PartKind};
use secrecy::{ExposeSecret, SecretString};
use serde_json::{Value, json};
use tracing::warn;

use crate::{
    config::Config,
    error::{RuntimeError, RuntimeResult},
};

use super::{AgentExecution, SharedDocId, current_doc_id};

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

#[derive(Clone)]
struct PreviewJob {
    id: u64,
    payload: EphemeralPayload,
    base_url: String,
    doc_id: String,
    token: SecretString,
    client: reqwest::Client,
}

impl PreviewJob {
    async fn send(self) -> RuntimeResult<bool> {
        let response = self
            .client
            .post(format!(
                "{}/api/docs/{}/ephemeral",
                self.base_url, self.doc_id
            ))
            .header(
                reqwest::header::AUTHORIZATION,
                format!("Bearer {}", self.token.expose_secret()),
            )
            .json(&self.payload)
            .send()
            .await;
        let response = response?;
        if !response.status().is_success() {
            return Err(RuntimeError::Internal(format!(
                "preview publish failed with {}",
                response.status()
            )));
        }
        Ok(true)
    }
}

struct PreviewSenderGuard(tokio::task::JoinHandle<()>);

impl Drop for PreviewSenderGuard {
    fn drop(&mut self) {
        self.0.abort();
    }
}

/// Latest-frame-wins asynchronous transport. Intermediate frames never block
/// the model stream; `flush` is the ordering barrier used before a real commit.
struct PreviewTransport {
    latest: watch::Sender<Option<PreviewJob>>,
    inflight: Arc<AtomicU64>,
    sent_tx: watch::Sender<u64>,
    sent_rx: watch::Receiver<u64>,
    base_url: String,
    doc_id: String,
    token: SecretString,
    client: reqwest::Client,
    task: Option<PreviewSenderGuard>,
}

impl PreviewTransport {
    fn new(base_url: &str, token: SecretString, doc_id: &str) -> Self {
        let (latest, _) = watch::channel::<Option<PreviewJob>>(None);
        let (sent_tx, sent_rx) = watch::channel(0_u64);
        let inflight = Arc::new(AtomicU64::new(0));
        let base_url = base_url.trim_end_matches('/').to_string();
        let doc_id = doc_id.to_string();
        let client = crate::http_client::timed_client(Duration::from_secs(30));
        Self {
            latest,
            inflight,
            sent_tx,
            sent_rx,
            base_url,
            doc_id,
            token,
            client,
            task: None,
        }
    }

    fn spawn_task(
        mut receiver: watch::Receiver<Option<PreviewJob>>,
        sent_tx: watch::Sender<u64>,
        inflight: Arc<AtomicU64>,
    ) -> PreviewSenderGuard {
        PreviewSenderGuard(tokio::spawn(async move {
            loop {
                if receiver.changed().await.is_err() {
                    break;
                }
                let Some(job) = receiver.borrow_and_update().clone() else {
                    continue;
                };
                let id = job.id;
                if let Err(error) = job.send().await {
                    warn!(%error, "ephemeral preview publish failed");
                }
                // `watch` may drop intermediate frames. Track the last completed
                // frame ID, not the number of completed sends, or latest-frame-wins
                // would make flush wait forever after an intermediate frame is replaced.
                sent_tx.send_replace(id);
            }
            // Closing the sender must not strand a caller waiting for the last
            // enqueued frame. Publish the current watermark before exiting.
            sent_tx.send_replace(inflight.load(Ordering::SeqCst));
        }))
    }

    fn enqueue(&mut self, payload: EphemeralPayload) {
        if self.task.is_none() {
            self.task = Some(Self::spawn_task(
                self.latest.subscribe(),
                self.sent_tx.clone(),
                Arc::clone(&self.inflight),
            ));
        }
        let id = self.inflight.fetch_add(1, Ordering::SeqCst) + 1;
        let job = PreviewJob {
            id,
            payload,
            base_url: self.base_url.clone(),
            doc_id: self.doc_id.clone(),
            token: self.token.clone(),
            client: self.client.clone(),
        };
        let _ = self.latest.send(Some(job));
    }

    async fn flush(&self) -> RuntimeResult<()> {
        let target = self.inflight.load(Ordering::SeqCst);
        let mut received = self.sent_rx.clone();
        tokio::time::timeout(
            Duration::from_secs(30),
            received.wait_for(|&sent| sent >= target),
        )
        .await
        .map_err(|_| RuntimeError::Internal("preview publish flush timed out".into()))?
        .map(|_| ())
        .map_err(|error| RuntimeError::Internal(format!("preview publish channel closed: {error}")))
    }
}

/// Sends partial tool-argument previews to Quire's ephemeral preview endpoint.
pub struct ToolPreviewStreamer {
    transport: PreviewTransport,
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
        let base_url = base_url.into();
        let doc_id = doc_id.into();
        Self {
            transport: PreviewTransport::new(&base_url, token, &doc_id),
            doc_id,
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
    /// monotonic sequence numbers. This only enqueues the latest frame and never waits for HTTP.
    pub fn push_block(&mut self, block: PartialBlockPreview) -> RuntimeResult<bool> {
        if !self.throttle.should_emit() {
            return Ok(false);
        }
        self.send_block(block)
    }

    /// Push the final frame even when the throttle window has not elapsed, then wait for the
    /// asynchronous sender to catch up. This is the ordering barrier before a real commit.
    pub(super) async fn push_block_now(
        &mut self,
        block: PartialBlockPreview,
    ) -> RuntimeResult<bool> {
        self.send_block(block)?;
        self.transport.flush().await?;
        Ok(true)
    }

    fn send_block(&mut self, block: PartialBlockPreview) -> RuntimeResult<bool> {
        let event = self.next_preview_event(block)?;
        self.transport.enqueue(EphemeralPayload::Preview { event });
        Ok(true)
    }
}

#[derive(Clone)]
struct AnchorContext {
    anchor: PreviewAnchor,
    /// `AppendToDoc` is meaningful only for an explicit `append` operation.
    /// Until then, a partial operation must not put its card at the doc tail.
    resolved: bool,
}

#[derive(Clone)]
struct PreviewBlock {
    block_id: Option<String>,
    block_type: String,
    title: Option<String>,
    level: Option<u8>,
    text: Option<String>,
    anchor: PreviewAnchor,
    anchor_resolved: bool,
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
    invocation_id: String,
    tool_name: String,
    buffer: String,
    // Argument position is stable while a block's id and anchor are still
    // arriving. Keying by the eventual block id would create a second card.
    streamers: HashMap<usize, ToolPreviewStreamer>,
    preview_ids: BTreeSet<String>,
}

pub(super) struct PreviewState {
    run_id: String,
    doc_id: SharedDocId,
    base_url: String,
    token: SecretString,
    actor: String,
    throttle_ms: u64,
    streams: HashMap<usize, StreamingPreview>,
    previews_by_call: HashMap<String, Vec<String>>,
    /// Preview IDs that have been handed off to `on_dispatch` and are awaiting
    /// confirmation in `on_outcome`. If the tool call fails or is aborted,
    /// these IDs are explicitly cleared via `EphemeralPayload::Clear`.
    dispatched_previews: HashMap<String, Vec<String>>,
}

impl PreviewState {
    pub(super) fn new(execution: &AgentExecution, config: &Config) -> Self {
        Self {
            run_id: execution.run_id.clone(),
            doc_id: Arc::clone(&execution.doc_id),
            base_url: config.quire_base_url.clone(),
            token: config.quire_rw_token.clone(),
            actor: execution.agent_id.clone(),
            throttle_ms: config.preview_throttle_ms,
            streams: HashMap::new(),
            previews_by_call: HashMap::new(),
            dispatched_previews: HashMap::new(),
        }
    }

    #[cfg(test)]
    pub(super) fn on_start(&mut self, part: usize, kind: PartKind, name: Option<&ToolName>) {
        if kind != PartKind::ToolCall {
            return;
        }
        let Some(name) = name else {
            return;
        };
        if !PREVIEW_TOOLS.contains(&name.as_str()) || current_doc_id(&self.doc_id).is_none() {
            return;
        }
        self.streams.insert(
            part,
            StreamingPreview {
                invocation_id: short_id("call"),
                tool_name: name.as_str().to_string(),
                buffer: String::new(),
                streamers: HashMap::new(),
                preview_ids: BTreeSet::new(),
            },
        );
    }

    #[cfg(test)]
    pub(super) async fn on_arguments(&mut self, part: usize, fragment: &str) {
        self.append_arguments(part, fragment).await;
    }

    /// Clear all in-flight and unconfirmed previews locally and broadcast an
    /// immediate `EphemeralPayload::Clear` to the Quire server so ghost cards
    /// never linger on the document canvas when a turn is interrupted or
    /// sibling tool calls are skipped by `ask_user`.
    pub(super) async fn abort_all(&mut self) {
        let mut ids = BTreeSet::new();
        for (_, stream) in self.streams.drain() {
            ids.extend(stream.preview_ids);
        }
        for (_, call_ids) in self.previews_by_call.drain() {
            ids.extend(call_ids);
        }
        for (_, call_ids) in self.dispatched_previews.drain() {
            ids.extend(call_ids);
        }
        self.publish_remote_clear(ids.into_iter().collect()).await;
    }

    /// Confirm that a dispatched tool call committed its revision (which
    /// atomically cleared its `preview_ids` on the server).
    pub(super) fn confirm_call(&mut self, call_id: &str) {
        self.previews_by_call.remove(call_id);
        self.dispatched_previews.remove(call_id);
    }

    /// Explicitly clear any previews associated with a tool call that failed
    /// or was skipped before committing a revision.
    pub(super) async fn abort_call(&mut self, call_id: &str) {
        let mut ids = BTreeSet::new();
        if let Some(call_ids) = self.previews_by_call.remove(call_id) {
            ids.extend(call_ids);
        }
        if let Some(call_ids) = self.dispatched_previews.remove(call_id) {
            ids.extend(call_ids);
        }
        self.publish_remote_clear(ids.into_iter().collect()).await;
    }

    async fn publish_remote_clear(&self, preview_ids: Vec<String>) {
        if preview_ids.is_empty() || self.base_url.trim().is_empty() {
            return;
        }
        let Some(raw_doc_id) = current_doc_id(&self.doc_id) else {
            return;
        };
        let Ok(doc_id) = DocId::parse(raw_doc_id.clone()) else {
            return;
        };
        let payload = EphemeralPayload::Clear {
            event: EphemeralClearEvent::new(doc_id, preview_ids),
        };
        let job = PreviewJob {
            id: 0,
            payload,
            base_url: self.base_url.trim_end_matches('/').to_string(),
            doc_id: raw_doc_id,
            token: self.token.clone(),
            client: crate::http_client::timed_client(Duration::from_secs(10)),
        };
        if let Err(error) = job.send().await {
            warn!(%error, "failed to publish ephemeral preview clear");
        }
    }

    pub(super) async fn on_tool_delta(
        &mut self,
        _turn: usize,
        part: usize,
        tool_name: &str,
        fragment: &str,
    ) {
        if !PREVIEW_TOOLS.contains(&tool_name) || current_doc_id(&self.doc_id).is_none() {
            return;
        }
        self.streams
            .entry(part)
            .or_insert_with(|| StreamingPreview {
                invocation_id: short_id("call"),
                tool_name: tool_name.to_string(),
                buffer: String::new(),
                streamers: HashMap::new(),
                preview_ids: BTreeSet::new(),
            });
        self.append_arguments(part, fragment).await;
    }

    async fn append_arguments(&mut self, part: usize, fragment: &str) {
        let Some(stream) = self.streams.get_mut(&part) else {
            return;
        };
        stream.buffer.push_str(fragment);
        let Some(doc_id) = current_doc_id(&self.doc_id) else {
            return;
        };
        let arguments = Value::Object(parse_partial_arguments(&stream.buffer));
        let blocks = extract_preview_blocks(&arguments);
        for (index, candidate) in blocks.iter().enumerate() {
            if !candidate.anchor_resolved {
                continue;
            }
            if !stream.streamers.contains_key(&index) {
                let block_id = candidate
                    .block_id
                    .clone()
                    .unwrap_or_else(|| format!("preview{part}{index}"));
                let key = candidate
                    .block_id
                    .clone()
                    .unwrap_or_else(|| format!("auto-{index}"));
                let preview_id = format!(
                    "{}:{}:{}:{}:{}",
                    self.run_id, stream.tool_name, stream.invocation_id, part, key
                );
                stream.streamers.insert(
                    index,
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
            if let Some(streamer) = stream.streamers.get_mut(&index) {
                streamer.block_id = candidate
                    .block_id
                    .clone()
                    .unwrap_or_else(|| format!("preview{part}{index}"));
                if streamer.anchor != candidate.anchor {
                    streamer.set_anchor(candidate.anchor.clone());
                }
                let block_id = BlockId::parse(
                    candidate
                        .block_id
                        .clone()
                        .unwrap_or_else(|| format!("preview{part}{index}")),
                )
                .unwrap_or_else(|_| BlockId::pending());
                match streamer.push_block(candidate.partial(block_id)) {
                    Ok(_) => {
                        stream.preview_ids.insert(preview_id_for(streamer));
                    }
                    Err(error) => {
                        tracing::warn!(%error, tool = %stream.tool_name, "ephemeral preview publish failed");
                    }
                }
            }
        }
    }

    pub(super) async fn finish_tool_call(
        &mut self,
        _turn: usize,
        call_id: &str,
        tool_name: &str,
        arguments: &Value,
    ) {
        let target_block_ids: BTreeSet<String> = extract_preview_blocks(arguments)
            .into_iter()
            .filter_map(|block| block.block_id)
            .collect();
        let matched_by_blocks = (!target_block_ids.is_empty())
            .then(|| {
                self.streams
                    .iter()
                    .filter(|(_, stream)| stream.tool_name == tool_name)
                    .filter_map(|(part, stream)| {
                        let overlap = stream
                            .streamers
                            .values()
                            .filter(|streamer| target_block_ids.contains(&streamer.block_id))
                            .count();
                        (overlap > 0).then_some((*part, overlap))
                    })
                    .max_by(|(left_part, left_overlap), (right_part, right_overlap)| {
                        left_overlap
                            .cmp(right_overlap)
                            .then_with(|| right_part.cmp(left_part))
                    })
                    .map(|(part, _)| part)
            })
            .flatten();
        let part = matched_by_blocks.or_else(|| {
            self.streams
                .iter()
                .filter(|(_, stream)| stream.tool_name == tool_name)
                .map(|(part, _)| *part)
                .min()
        });
        if let Some(part) = part {
            self.finalize_part(part, call_id, arguments).await;
        }
    }

    async fn finalize_part(&mut self, part: usize, call_id: &str, arguments: &Value) {
        let Some(mut stream) = self.streams.remove(&part) else {
            return;
        };
        let Some(doc_id) = current_doc_id(&self.doc_id) else {
            return;
        };
        // The model may emit `blocks` before `target_id`/`section_id`/`type`. Frames sent
        // while those fields are missing carry the provisional `AppendToDoc` anchor. Once
        // the complete arguments arrive, republish every block with its resolved anchor and
        // give the frontend one paint window before the commit clears the previews.
        let mut corrected = false;
        let final_blocks = extract_preview_blocks(arguments);
        for (index, candidate) in final_blocks.iter().enumerate() {
            if !candidate.anchor_resolved {
                continue;
            }
            if !stream.streamers.contains_key(&index) {
                let block_id = candidate
                    .block_id
                    .clone()
                    .unwrap_or_else(|| format!("preview{part}{index}"));
                let key = candidate
                    .block_id
                    .clone()
                    .unwrap_or_else(|| format!("auto-{index}"));
                let preview_id = format!(
                    "{}:{}:{}:{}:{}",
                    self.run_id, stream.tool_name, stream.invocation_id, part, key
                );
                stream.streamers.insert(
                    index,
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
            let Some(streamer) = stream.streamers.get_mut(&index) else {
                continue;
            };
            streamer.block_id = candidate
                .block_id
                .clone()
                .unwrap_or_else(|| format!("preview{part}{index}"));
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
                call_id.to_string(),
                stream.preview_ids.into_iter().collect(),
            );
        }
    }

    pub(super) fn take_preview_ids(&mut self, call_id: &str) -> Option<Vec<String>> {
        let ids = self.previews_by_call.remove(call_id)?;
        self.dispatched_previews
            .insert(call_id.to_string(), ids.clone());
        Some(ids)
    }

    #[cfg(test)]
    pub(super) fn for_test(run_id: &str, doc_id: SharedDocId) -> Self {
        Self {
            run_id: run_id.to_string(),
            doc_id,
            base_url: String::new(),
            token: SecretString::from(String::new()),
            actor: "test".into(),
            throttle_ms: 0,
            streams: HashMap::new(),
            previews_by_call: HashMap::new(),
            dispatched_previews: HashMap::new(),
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
        &AnchorContext {
            anchor: PreviewAnchor::AppendToDoc,
            resolved: false,
        },
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
    inherited: &AnchorContext,
) -> AnchorContext {
    let op_type = operation_type(object);
    if op_type == Some("append") {
        return AnchorContext {
            anchor: PreviewAnchor::AppendToDoc,
            resolved: true,
        };
    }
    if op_type == Some("append_to_section") || object.contains_key("section_id") {
        if let Some(section_id) =
            parse_preview_block_id(object.get("section_id").and_then(Value::as_str))
        {
            return AnchorContext {
                anchor: PreviewAnchor::AppendToSection { section_id },
                resolved: true,
            };
        }
        return AnchorContext {
            anchor: inherited.anchor.clone(),
            resolved: false,
        };
    }
    if op_type == Some("insert_after") {
        if let Some(block_id) =
            parse_preview_block_id(object.get("target_id").and_then(Value::as_str))
        {
            return AnchorContext {
                anchor: PreviewAnchor::AfterBlock { block_id },
                resolved: true,
            };
        }
        return AnchorContext {
            anchor: inherited.anchor.clone(),
            resolved: false,
        };
    }
    if op_type == Some("insert_before") {
        if let Some(block_id) =
            parse_preview_block_id(object.get("target_id").and_then(Value::as_str))
        {
            return AnchorContext {
                anchor: PreviewAnchor::BeforeBlock { block_id },
                resolved: true,
            };
        }
        return AnchorContext {
            anchor: inherited.anchor.clone(),
            resolved: false,
        };
    }
    if is_patch_operation(op_type, object) {
        if let Some(block_id) =
            parse_preview_block_id(object.get("block_id").and_then(Value::as_str))
        {
            return AnchorContext {
                anchor: PreviewAnchor::ReplaceBlock { block_id },
                resolved: true,
            };
        }
        return AnchorContext {
            anchor: inherited.anchor.clone(),
            resolved: false,
        };
    }
    if op_type == Some("replace_range") {
        if let Some(block_id) =
            parse_preview_block_id(object.get("start_id").and_then(Value::as_str))
        {
            return AnchorContext {
                anchor: PreviewAnchor::ReplaceBlock { block_id },
                resolved: true,
            };
        }
        return AnchorContext {
            anchor: inherited.anchor.clone(),
            resolved: false,
        };
    }
    inherited.clone()
}

fn collect_preview_blocks(
    value: &Value,
    depth: usize,
    anchor: &AnchorContext,
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
            let is_container =
                object.contains_key("ops") || object.contains_key("blocks") || patch_op;
            if !is_container && let Some(block) = preview_block(object, &next_anchor) {
                push_preview_block(block, blocks, seen);
                return;
            }
            let chained_insert = matches!(op_type, Some("insert_after" | "insert_before"));
            for (key, child) in object {
                if key == "patch" && consumed_patch {
                    continue;
                }
                // Never recurse into block-internal payload structures as if they were
                // child document blocks (e.g., ECharts `option.title.text` or table cells).
                if matches!(
                    key.as_str(),
                    "option"
                        | "option_patch"
                        | "rows"
                        | "columns"
                        | "sources"
                        | "source_ids"
                        | "numbering"
                ) {
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
    anchor: &AnchorContext,
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
            chain_anchor = AnchorContext {
                anchor: PreviewAnchor::AfterBlock { block_id },
                resolved: chain_anchor.resolved,
            };
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

/// Synthesize a compact pipe-separated table preview (`Col A | Col B\nRow1A | Row1B`)
/// from partial `columns` and `rows` while a `table` block is streaming.
fn synthesize_table_preview_text(object: &serde_json::Map<String, Value>) -> Option<String> {
    let columns = object.get("columns").and_then(Value::as_array);
    let rows = object.get("rows").and_then(Value::as_array);
    let mut column_keys: Vec<&str> = Vec::new();
    let mut header_labels: Vec<String> = Vec::new();
    if let Some(cols) = columns {
        for col in cols.iter().take(6) {
            let Some(col_obj) = col.as_object() else {
                continue;
            };
            if let Some(key) = col_obj.get("key").and_then(Value::as_str) {
                column_keys.push(key);
            }
            let label = col_obj
                .get("label")
                .or_else(|| col_obj.get("title"))
                .or_else(|| col_obj.get("key"))
                .and_then(Value::as_str)
                .map(str::trim)
                .filter(|value| !value.is_empty());
            if let Some(label) = label {
                header_labels.push(label.to_string());
            }
        }
    }
    let mut lines: Vec<String> = Vec::new();
    if !header_labels.is_empty() {
        lines.push(header_labels.join(" | "));
    }
    if let Some(row_list) = rows {
        for row in row_list.iter().take(4) {
            let cells: Vec<String> = match row {
                Value::Object(row_obj) => {
                    if !column_keys.is_empty() {
                        column_keys
                            .iter()
                            .map(|key| format_cell_preview(row_obj.get(*key)))
                            .collect()
                    } else {
                        row_obj
                            .values()
                            .take(6)
                            .map(|val| format_cell_preview(Some(val)))
                            .collect()
                    }
                }
                Value::Array(arr) => arr
                    .iter()
                    .take(6)
                    .map(|val| format_cell_preview(Some(val)))
                    .collect(),
                _ => Vec::new(),
            };
            if !cells.is_empty() && cells.iter().any(|cell| !cell.is_empty()) {
                lines.push(cells.join(" | "));
            }
        }
        if row_list.len() > 4 {
            lines.push(format!("… 共 {} 行数据流式写入中", row_list.len()));
        }
    }
    if lines.is_empty() {
        object
            .get("note")
            .and_then(Value::as_str)
            .map(str::to_string)
    } else {
        Some(lines.join("\n"))
    }
}

fn format_cell_preview(value: Option<&Value>) -> String {
    match value {
        Some(Value::String(text)) => text.trim().chars().take(28).collect(),
        Some(Value::Number(num)) => num.to_string(),
        Some(Value::Bool(flag)) => flag.to_string(),
        Some(Value::Object(obj)) => obj
            .get("text")
            .or_else(|| obj.get("value"))
            .and_then(Value::as_str)
            .map(|text| text.trim().chars().take(28).collect())
            .unwrap_or_default(),
        _ => String::new(),
    }
}

fn preview_block(
    object: &serde_json::Map<String, Value>,
    anchor: &AnchorContext,
) -> Option<PreviewBlock> {
    let inferred_type = if object.contains_key("level") {
        "heading"
    } else if object.contains_key("rows")
        || object.contains_key("columns")
        || object.contains_key("caption")
    {
        "table"
    } else if object.contains_key("option")
        || object.contains_key("option_patch")
        || object.contains_key("fallback_summary")
    {
        "echarts"
    } else if object.contains_key("html") || object.contains_key("fallback_text") {
        "custom_html"
    } else if object.contains_key("attribution") {
        "quote"
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

    let title = object
        .get("title")
        .or_else(|| object.get("caption"))
        .or_else(|| {
            (block_type == "quote")
                .then(|| object.get("attribution"))
                .flatten()
        })
        .and_then(Value::as_str)
        .map(str::to_string)
        .or_else(|| {
            (block_type == "echarts")
                .then(|| {
                    object
                        .get("option")
                        .and_then(Value::as_object)
                        .and_then(|opt| opt.get("title"))
                        .and_then(Value::as_object)
                        .and_then(|t| t.get("text"))
                        .and_then(Value::as_str)
                        .map(str::to_string)
                })
                .flatten()
        });

    let text = object
        .get("text")
        .and_then(Value::as_str)
        .map(str::to_string)
        .or_else(|| match block_type {
            "table" => synthesize_table_preview_text(object),
            "echarts" => object
                .get("fallback_summary")
                .or_else(|| object.get("note"))
                .and_then(Value::as_str)
                .map(str::to_string),
            "custom_html" => object
                .get("fallback_text")
                .or_else(|| object.get("html"))
                .and_then(Value::as_str)
                .map(str::to_string),
            _ => None,
        });

    if text.is_none() && title.is_none() {
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
        title,
        level,
        text,
        anchor: anchor.anchor.clone(),
        anchor_resolved: anchor.resolved,
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
    use super::super::{new_shared_doc_id, update_doc_id};
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
    fn table_and_echarts_blocks_produce_single_rich_previews_without_inner_duplicates() {
        let arguments = json!({
            "ops": [{
                "type": "append",
                "blocks": [
                    {
                        "id": "ch1_table",
                        "type": "table",
                        "caption": "核心学科经费对比",
                        "columns": [
                            {"key": "school", "label": "高校"},
                            {"key": "budget", "label": "经费（亿元）"}
                        ],
                        "rows": [
                            {"school": "A校", "budget": "14.2"},
                            {"school": "B校", "budget": "11.8"}
                        ]
                    },
                    {
                        "id": "ch1_chart",
                        "type": "echarts",
                        "title": "近五年经费趋势",
                        "fallback_summary": "A校近五年持续增长",
                        "option": {
                            "title": {"text": "内部子标题不应生成独立段落块"},
                            "series": [{"type": "line", "data": [10, 12, 14]}]
                        }
                    }
                ]
            }]
        });
        let blocks = extract_preview_blocks(&arguments);
        assert_eq!(
            blocks.len(),
            2,
            "nested option.title.text or table columns must not create extra blocks: {:?}",
            blocks
                .iter()
                .map(|b| (&b.block_id, &b.block_type))
                .collect::<Vec<_>>()
        );
        assert_eq!(blocks[0].block_id.as_deref(), Some("ch1_table"));
        assert_eq!(blocks[0].block_type, "table");
        assert_eq!(blocks[0].title.as_deref(), Some("核心学科经费对比"));
        assert!(blocks[0].text.as_deref().is_some_and(
            |text| text.contains("高校 | 经费（亿元）") && text.contains("A校 | 14.2")
        ));
        assert_eq!(blocks[1].block_id.as_deref(), Some("ch1_chart"));
        assert_eq!(blocks[1].block_type, "echarts");
        assert_eq!(blocks[1].title.as_deref(), Some("近五年经费趋势"));
        assert_eq!(blocks[1].text.as_deref(), Some("A校近五年持续增长"));
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
    fn incomplete_anchors_stay_deferred_but_explicit_append_is_resolved() {
        let partial = json!({
            "ops": [{"blocks": [{"text": "draft"}]}]
        });
        let blocks = extract_preview_blocks(&partial);
        assert_eq!(blocks.len(), 1);
        assert!(!blocks[0].anchor_resolved);

        let explicit = json!({
            "ops": [{"type": "append", "blocks": [{"text": "draft"}]}]
        });
        let blocks = extract_preview_blocks(&explicit);
        assert_eq!(blocks.len(), 1);
        assert!(blocks[0].anchor_resolved);
        assert_eq!(blocks[0].anchor, PreviewAnchor::AppendToDoc);
    }

    #[tokio::test]
    async fn preview_identity_is_unique_per_tool_call_and_stable_while_id_arrives() {
        let tool = ToolName::new("apply_operations").expect("tool name");
        let partial = r#"{"ops":[{"type":"append_to_section","section_id":"sec1","blocks":[{"type":"paragraph","text":"draft""#;
        let suffix = r#","id":"sec1_p"}]}]}"#;
        let complete = r#"{"ops":[{"type":"append_to_section","section_id":"sec1","blocks":[{"type":"paragraph","text":"draft","id":"sec1_p"}]}]}"#;

        let mut first = PreviewState::for_test("run_test", new_shared_doc_id(Some("doc".into())));
        first.on_start(1, PartKind::ToolCall, Some(&tool));
        first.on_arguments(1, partial).await;
        let first_id = first.streams[&1].streamers[&0].preview_id().to_string();
        first.on_arguments(1, suffix).await;
        let stream = &first.streams[&1];
        assert_eq!(stream.streamers.len(), 1);
        assert_eq!(stream.streamers[&0].preview_id(), first_id);
        assert_eq!(stream.streamers[&0].block_id, "sec1_p");

        let mut second = PreviewState::for_test("run_test", new_shared_doc_id(Some("doc".into())));
        second.on_start(1, PartKind::ToolCall, Some(&tool));
        second.on_arguments(1, complete).await;
        let second_id = second.streams[&1].streamers[&0].preview_id();
        assert_ne!(first_id, second_id);
    }

    #[tokio::test]
    async fn lazy_document_binding_enables_subsequent_tool_deltas() {
        let doc_id = new_shared_doc_id(None);
        let mut state = PreviewState::for_test("run_test", Arc::clone(&doc_id));
        state
            .on_tool_delta(1, 7, "apply_operations", r#"{"ops":[]}"#)
            .await;
        assert!(!state.streams.contains_key(&7));

        update_doc_id(&doc_id, Some("doc_lazy".into()));
        state
            .on_tool_delta(1, 7, "apply_operations", r#"{"ops":[]}"#)
            .await;
        assert!(state.streams.contains_key(&7));
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

#[cfg(test)]
mod handoff_tests {
    use super::*;
    use std::sync::Arc;

    use axum::{Json, Router, extract::State, http::StatusCode, routing::post};
    use tokio::sync::Mutex;

    async fn record_preview(
        State(requests): State<Arc<Mutex<Vec<Value>>>>,
        Json(payload): Json<Value>,
    ) -> StatusCode {
        let mut requests = requests.lock().await;
        let delay = if requests.is_empty() { 200 } else { 0 };
        requests.push(payload);
        drop(requests);
        if delay > 0 {
            tokio::time::sleep(Duration::from_millis(delay)).await;
        }
        StatusCode::OK
    }

    pub(super) async fn spawn_server() -> (String, Arc<Mutex<Vec<Value>>>) {
        let requests = Arc::new(Mutex::new(Vec::new()));
        let app = Router::new()
            .route("/api/docs/doc/ephemeral", post(record_preview))
            .with_state(Arc::clone(&requests));
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let address = listener.local_addr().unwrap();
        tokio::spawn(async move {
            let _ = axum::serve(listener, app).await;
        });
        (format!("http://{address}"), requests)
    }

    pub(super) fn block(text: &str) -> PartialBlockPreview {
        PartialBlockPreview {
            block_id: Some(BlockId::pending()),
            block_type: "paragraph".into(),
            title: None,
            level: None,
            text: Some(text.to_string()),
        }
    }

    fn preview_text(payload: &Value) -> String {
        payload
            .get("event")
            .unwrap_or(payload)
            .get("block")
            .and_then(|block| block.get("text"))
            .and_then(Value::as_str)
            .unwrap_or_default()
            .to_string()
    }

    #[tokio::test]
    async fn intermediate_frames_are_latest_wins_and_flush_waits_for_the_final_frame() {
        let (base_url, requests) = spawn_server().await;
        let mut streamer = ToolPreviewStreamer::new(
            &base_url,
            SecretString::from("test-token"),
            "doc",
            "preview-1",
            "main",
            "block-1",
            50,
        );

        streamer.push_block(block("first")).unwrap();
        streamer.push_block(block("second")).unwrap();
        streamer.push_block(block("third")).unwrap();
        tokio::time::timeout(
            Duration::from_secs(2),
            streamer.push_block_now(block("final")),
        )
        .await
        .unwrap()
        .unwrap();

        let requests = requests.lock().await;
        let texts = requests.iter().map(preview_text).collect::<Vec<_>>();
        assert_eq!(texts.last().map(String::as_str), Some("final"));
        assert!(
            texts.len() <= 2,
            "latest-frame-wins must not replay every intermediate frame: {texts:?}"
        );
    }
}

#[cfg(test)]
mod flush_wakeup_tests {
    use super::{
        handoff_tests::{block, spawn_server},
        *,
    };

    #[tokio::test]
    async fn flush_returns_immediately_once_the_last_frame_is_sent() {
        let (base_url, _requests) = spawn_server().await;
        let mut streamer = ToolPreviewStreamer::new(
            &base_url,
            SecretString::from("test-token"),
            "doc",
            "preview-wakeup",
            "main",
            "block-1",
            50,
        );
        streamer.push_block(block("first")).unwrap();
        tokio::time::sleep(Duration::from_millis(250)).await;
        tokio::time::timeout(Duration::from_millis(100), streamer.transport.flush())
            .await
            .unwrap()
            .unwrap();
    }
}
