//! Server-Sent Events for revision deltas, session discovery and ephemeral previews.

use std::convert::Infallible;

use axum::response::sse::Event;
use futures::Stream;
use quire_protocol::{DocId, Revision, SessionId, SyncRequiredDelta};
use quire_store::{SessionStreamEntry, StreamRevision};
use serde_json::Value;
use tokio::sync::mpsc;
use tokio::task::JoinHandle;
use tracing::{debug, warn};

use crate::state::AppState;

struct AbortOnDrop(JoinHandle<()>);

impl Drop for AbortOnDrop {
    fn drop(&mut self) {
        self.0.abort();
    }
}

enum LiveMessage {
    Revision(StreamRevision),
    Error(String),
}

enum EphemeralMessage {
    Payload(String),
}

fn sse_json(event_name: &str, value: &impl serde::Serialize) -> Event {
    Event::default()
        .event(event_name)
        .json_data(value)
        .unwrap_or_else(|error| {
            Event::default()
                .event("error")
                .data(format!("serialization error: {error}"))
        })
}

fn sync_required(doc_id: &DocId, since: Revision, reason: &str) -> Event {
    let payload = SyncRequiredDelta {
        doc_id: doc_id.clone(),
        since_revision: since,
        reason: reason.to_string(),
        hint: "Reconnect and fetch GET /api/docs/{doc_id} for a full snapshot before resuming SSE."
            .to_string(),
    };
    Event::default()
        .event("sync_required")
        .json_data(payload)
        .unwrap_or_else(|_| Event::default().event("sync_required").data("{}"))
}

/// Replay missed revisions (Redis first, PostgreSQL fallback), then stream live deltas.
pub fn revision_stream(
    state: AppState,
    doc_id: DocId,
    since_rev: Revision,
    current_rev: Revision,
) -> impl Stream<Item = Result<Event, Infallible>> {
    async_stream::stream! {
        yield Ok(Event::default().comment("connected"));
        let bus = state.store.bus().clone();
        let heartbeat_period = state.config.sse_heartbeat;
        let stream_start = bus
            .latest_revision_stream_id(doc_id.as_str())
            .await
            .unwrap_or_else(|error| {
                warn!(%error, %doc_id, "failed to read Redis stream head; falling back to 0-0");
                "0-0".to_string()
            });

        // Subscribe before replaying so no ephemeral previews are lost during the snapshot.
        let (ephemeral_tx, mut ephemeral_rx) = mpsc::channel(128);
        // Keep a sender alive even if the subscription cannot be established, preventing a
        // busy loop on `recv() -> None` while revisions/heartbeats continue.
        let ephemeral_keepalive = ephemeral_tx.clone();
        let ephemeral_guard = match bus.subscribe_ephemeral(&doc_id).await {
            Ok(mut subscriber) => Some(AbortOnDrop(tokio::spawn(async move {
                while let Some(payload) = subscriber.next_payload().await {
                    if ephemeral_tx
                        .send(EphemeralMessage::Payload(payload))
                        .await
                        .is_err()
                    {
                        return;
                    }
                }
            }))),
            Err(error) => {
                warn!(%error, doc_id = doc_id.as_str(), "ephemeral subscription unavailable");
                None
            }
        };

        let mut last_sent_rev = since_rev;
        let mut last_stream_id = stream_start.clone();
        let mut replayed_via_redis = false;

        // Prefer Redis when it contains the immediately expected revision and no gaps.
        let redis_revisions = bus.read_revisions(doc_id.as_str()).await.unwrap_or_default();
        let relevant: Vec<&StreamRevision> = redis_revisions
            .iter()
            .filter(|entry| entry.event.revision > since_rev)
            .collect();
        let expected_next = since_rev.next();
        let contiguous_redis = relevant
            .iter()
            .enumerate()
            .all(|(index, entry)| entry.event.revision.get() == expected_next.get() + index as i64);
        let covers_expected = relevant
            .first()
            .is_some_and(|entry| entry.event.revision == expected_next);
        if (covers_expected || current_rev < expected_next) && contiguous_redis {
            replayed_via_redis = !relevant.is_empty();
            for entry in relevant {
                if entry.event.revision <= last_sent_rev {
                    continue;
                }
                last_sent_rev = entry.event.revision;
                last_stream_id = entry.stream_id.clone();
                yield Ok(sse_json("revision", &entry.event));
            }
        }

        if !replayed_via_redis {
            // PostgreSQL is the durable source of truth. Paginate all missing revisions.
            let mut cursor = since_rev;
            let mut gap_detected = false;
            loop {
                match state
                    .store
                    .revision_events_since_pg(&doc_id, cursor, 500)
                    .await
                {
                    Ok(events) if events.is_empty() => break,
                    Ok(events) => {
                        for event in events {
                            if event.revision.get() > last_sent_rev.get() + 1
                                && last_sent_rev.get() + 1 >= 1
                            {
                                gap_detected = true;
                            }
                            if event.revision <= last_sent_rev {
                                continue;
                            }
                            last_sent_rev = event.revision;
                            cursor = event.revision;
                            yield Ok(sse_json("revision", &event));
                        }
                        if last_sent_rev >= current_rev {
                            break;
                        }
                    }
                    Err(error) => {
                        warn!(%error, doc_id = doc_id.as_str(), "PostgreSQL replay failed");
                        yield Ok(sync_required(
                            &doc_id,
                            last_sent_rev,
                            "durable revision replay failed",
                        ));
                        break;
                    }
                }
            }
            if gap_detected {
                yield Ok(sync_required(
                    &doc_id,
                    last_sent_rev,
                    "revision history has a gap in the durable log",
                ));
            }
        }

        // Live reader on a dedicated connection.
        let (revision_tx, mut revision_rx) = mpsc::channel(256);
        let reader_bus = bus.clone();
        let reader_doc = doc_id.clone();
        let reader_start_id = last_stream_id.clone();
        let reader_guard = AbortOnDrop(tokio::spawn(async move {
            let mut reader = match reader_bus.blocking_reader().await {
                Ok(reader) => reader,
                Err(error) => {
                    let _ = revision_tx.send(LiveMessage::Error(error.to_string())).await;
                    return;
                }
            };
            let mut last_id = reader_start_id;
            loop {
                match reader.read_revisions(&reader_doc, &last_id, 5000).await {
                    Ok(entries) => {
                        if entries.is_empty() {
                            continue;
                        }
                        for entry in entries {
                            last_id = entry.stream_id.clone();
                            if revision_tx
                                .send(LiveMessage::Revision(entry))
                                .await
                                .is_err()
                            {
                                return;
                            }
                        }
                    }
                    Err(error) => {
                        let _ = revision_tx.send(LiveMessage::Error(error.to_string())).await;
                        return;
                    }
                }
            }
        }));

        let started_at = tokio::time::Instant::now() + heartbeat_period;
        let mut heartbeat = tokio::time::interval_at(started_at, heartbeat_period);
        loop {
            tokio::select! {
                message = revision_rx.recv() => {
                    match message {
                        Some(LiveMessage::Revision(entry)) => {
                            if entry.event.revision <= last_sent_rev {
                                continue;
                            }
                            last_sent_rev = entry.event.revision;
                            yield Ok(sse_json("revision", &entry.event));
                        }
                        Some(LiveMessage::Error(error)) => {
                            warn!(%error, doc_id = doc_id.as_str(), "live revision stream failed");
                            yield Ok(sync_required(&doc_id, last_sent_rev, "live Redis stream failed"));
                            break;
                        }
                        None => break,
                    }
                }
                message = ephemeral_rx.recv() => {
                    let Some(EphemeralMessage::Payload(payload)) = message else { continue };
                    let value = serde_json::from_str::<Value>(&payload)
                        .unwrap_or(Value::String(payload));
                    let kind = value.get("kind").and_then(Value::as_str).unwrap_or("ephemeral");
                    let event_name = if matches!(kind, "presence" | "agent_presence") {
                        "agent_presence"
                    } else {
                        "ephemeral_preview"
                    };
                    debug!(doc_id = doc_id.as_str(), event_name, "forwarding ephemeral SSE frame");
                    yield Ok(sse_json(event_name, &value));
                }
                _ = heartbeat.tick() => {
                    yield Ok(Event::default().comment("heartbeat"));
                }
            }
        }
        drop(reader_guard);
        drop(ephemeral_guard);
        drop(ephemeral_keepalive);
    }
}

/// Replay session discovery events, then stream new `doc_created` frames.
pub fn session_stream(
    state: AppState,
    session_id: SessionId,
) -> impl Stream<Item = Result<Event, Infallible>> {
    async_stream::stream! {
        yield Ok(Event::default().comment("connected"));
        let bus = state.store.bus().clone();
        let heartbeat_period = state.config.sse_heartbeat;
        let mut last_stream_id = "0-0".to_string();
        match bus.read_session_events(session_id.as_str()).await {
            Ok(events) => {
                for entry in events {
                    last_stream_id = entry.stream_id.clone();
                    yield Ok(sse_json(&entry.event.event, &entry.event));
                }
            }
            Err(error) => {
                warn!(%error, session_id = session_id.as_str(), "session replay failed");
            }
        }

        let (tx, mut rx) = mpsc::channel::<SessionStreamEntry>(64);
        let reader_bus = bus.clone();
        let reader_session = session_id.clone();
        let reader_start_id = last_stream_id.clone();
        let reader_guard = AbortOnDrop(tokio::spawn(async move {
            let mut reader = match reader_bus.blocking_reader().await {
                Ok(reader) => reader,
                Err(error) => {
                    warn!(%error, session_id = reader_session.as_str(), "session live reader unavailable");
                    return;
                }
            };
            let mut last_id = reader_start_id;
            loop {
                match reader
                    .read_session_events(reader_session.as_str(), &last_id, 5000)
                    .await
                {
                    Ok(events) => {
                        for event in events {
                            last_id = event.stream_id.clone();
                            if tx.send(event).await.is_err() {
                                return;
                            }
                        }
                    }
                    Err(error) => {
                        warn!(%error, session_id = reader_session.as_str(), "session live stream failed");
                        return;
                    }
                }
            }
        }));

        let started_at = tokio::time::Instant::now() + heartbeat_period;
        let mut heartbeat = tokio::time::interval_at(started_at, heartbeat_period);
        loop {
            tokio::select! {
                message = rx.recv() => {
                    let Some(entry) = message else { break };
                    yield Ok(sse_json(&entry.event.event, &entry.event));
                }
                _ = heartbeat.tick() => {
                    yield Ok(Event::default().comment("heartbeat"));
                }
            }
        }
        drop(reader_guard);
    }
}

/// Session replay helper used by unit tests.
pub fn session_event_name(event: &SessionStreamEntry) -> &str {
    &event.event.event
}

/// Build a minimal SSE frame for the ephemeral endpoint response documentation.
pub fn preview_event_name(payload: &Value) -> &'static str {
    match payload.get("kind").and_then(Value::as_str) {
        Some("presence" | "agent_presence") => "agent_presence",
        _ => "ephemeral_preview",
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn preview_kind_mapping() {
        assert_eq!(
            preview_event_name(&serde_json::json!({"kind":"presence"})),
            "agent_presence"
        );
        assert_eq!(
            preview_event_name(&serde_json::json!({"block_id":"b"})),
            "ephemeral_preview"
        );
    }
}
