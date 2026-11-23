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
use serde_json::Value;
#[cfg(test)]
use serde_json::json;
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
    report_store: Option<quire_store::PgReportStore>,
    base_url: String,
    doc_id: String,
    token: SecretString,
    client: reqwest::Client,
}

impl PreviewJob {
    async fn send(mut self) -> RuntimeResult<bool> {
        if let Some(store) = self.report_store.as_ref() {
            let doc_id = DocId::parse(self.doc_id.clone())
                .map_err(|error| RuntimeError::InvalidRequest(error.to_string()))?;
            self.payload
                .stamp_server_context(&doc_id, chrono::Utc::now());
            self.payload
                .validate()
                .map_err(RuntimeError::InvalidRequest)?;
            store
                .bus()
                .publish_ephemeral(doc_id.as_str(), &self.payload)
                .await
                .map_err(|error| RuntimeError::Internal(error.to_string()))?;
            return Ok(true);
        }
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
    report_store: Option<quire_store::PgReportStore>,
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
            report_store: None,
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
            report_store: self.report_store.clone(),
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

    pub(super) fn with_report_store(
        mut self,
        report_store: Option<quire_store::PgReportStore>,
    ) -> Self {
        self.transport.report_store = report_store;
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
pub(crate) struct PreviewBlock {
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
    report_store: Option<quire_store::PgReportStore>,
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
    pub(super) fn new(
        execution: &AgentExecution,
        config: &Config,
        report_store: Option<quire_store::PgReportStore>,
    ) -> Self {
        Self {
            run_id: execution.run_id.clone(),
            doc_id: Arc::clone(&execution.doc_id),
            report_store,
            base_url: config.quire_base_url.clone(),
            token: SecretString::from(String::new()),
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
        if preview_ids.is_empty() {
            return;
        }
        let Some(raw_doc_id) = current_doc_id(&self.doc_id) else {
            return;
        };
        let Ok(doc_id) = DocId::parse(raw_doc_id.clone()) else {
            return;
        };
        let mut payload = EphemeralPayload::Clear {
            event: EphemeralClearEvent::new(doc_id.clone(), preview_ids),
        };
        if let Some(store) = self.report_store.as_ref() {
            payload.stamp_server_context(&doc_id, chrono::Utc::now());
            if let Err(error) = store
                .bus()
                .publish_ephemeral(doc_id.as_str(), &payload)
                .await
            {
                warn!(%error, "failed to publish ephemeral preview clear");
            }
            return;
        }
        if self.base_url.trim().is_empty() {
            return;
        }
        let job = PreviewJob {
            id: 0,
            payload,
            report_store: None,
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
}

mod projection;
mod streamer;
#[cfg(test)]
mod tests;

pub(crate) use projection::{extract_preview_blocks, preview_id_for};
pub(crate) use streamer::inject_preview_ids;
