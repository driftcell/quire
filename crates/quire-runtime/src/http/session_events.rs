use crate::auth::{AuthUser, UserRole};
use axum::Extension;

use super::*;

#[derive(Debug, Deserialize)]
pub(crate) struct EventsQuery {
    #[serde(rename = "afterSeq", alias = "after_seq")]
    after_seq: Option<u64>,
}

#[derive(Debug, Deserialize)]
pub(crate) struct PruneEventsRequest {
    keep_events: Option<usize>,
}

#[derive(Debug, Serialize)]
pub(crate) struct PruneEventsResponse {
    deleted: u64,
    min_available_seq: u64,
}

pub(crate) async fn prune_events(
    State(service): State<RuntimeService>,
    Extension(auth_user): Extension<AuthUser>,
    Path(session_id): Path<String>,
    Json(request): Json<PruneEventsRequest>,
) -> RuntimeResult<Json<PruneEventsResponse>> {
    service
        .ensure_session_access(
            &session_id,
            &auth_user.user_id,
            auth_user.role == UserRole::Admin,
        )
        .await?;
    let (deleted, min_available_seq) = service
        .store()
        .prune_session_events(&session_id, request.keep_events.unwrap_or(1_000))
        .await?;
    Ok(Json(PruneEventsResponse {
        deleted,
        min_available_seq,
    }))
}

pub(crate) async fn session_events(
    State(service): State<RuntimeService>,
    Extension(auth_user): Extension<AuthUser>,
    Path(session_id): Path<String>,
    Query(query): Query<EventsQuery>,
) -> Result<Sse<impl Stream<Item = Result<Event, Infallible>>>, RuntimeError> {
    service
        .ensure_session_access(
            &session_id,
            &auth_user.user_id,
            auth_user.role == UserRole::Admin,
        )
        .await?;
    let summary = service.store().get_session_summary(&session_id).await?;
    let min_available_seq = service.store().min_available_seq(&session_id).await?;
    let after_seq = query.after_seq.unwrap_or(0);
    let stream = event_stream(
        service,
        session_id,
        after_seq,
        summary.last_event_seq,
        min_available_seq,
    );
    Ok(Sse::new(stream).keep_alive(KeepAlive::default()))
}

fn event_stream(
    service: RuntimeService,
    session_id: String,
    after_seq: u64,
    server_last_seq: u64,
    min_available_seq: u64,
) -> impl Stream<Item = Result<Event, Infallible>> {
    async_stream::stream! {
        yield Ok(Event::default().comment("connected"));
        if after_seq > server_last_seq {
            yield Ok(sse_event(&synthetic_sync(
                &session_id,
                after_seq,
                "client cursor is ahead of the server",
            )));
            return;
        }
        if after_seq.saturating_add(1) < min_available_seq {
            yield Ok(sse_event(&synthetic_sync(
                &session_id,
                after_seq,
                "replay cursor is older than the retained event window",
            )));
            return;
        }
        let store = service.store().clone();
        let bus = store.bus().clone();
        let replay_limit = service.config().sse_replay_limit;
        let total = store.count_events_after(&session_id, after_seq).await.unwrap_or(0);
        if u64::try_from(total).unwrap_or(u64::MAX) > u64::try_from(replay_limit).unwrap_or(u64::MAX) {
            yield Ok(sse_event(&synthetic_sync(&session_id, after_seq, "replay window exceeded")));
            return;
        }
        let mut subscriber = match bus.subscribe_events(&session_id).await {
            Ok(subscriber) => subscriber,
            Err(error) => {
                yield Ok(sse_event(&synthetic_sync(
                    &session_id,
                    after_seq,
                    &format!("live subscription unavailable: {error}"),
                )));
                return;
            }
        };
        let mut last_seq = after_seq;
        match store.list_event_frames(&session_id, after_seq, i64::try_from(replay_limit).unwrap_or(500)).await {
            Ok(frames) => {
                for frame in frames {
                    last_seq = frame.seq;
                    yield Ok(sse_frame(&frame));
                }
            }
            Err(error) => {
                yield Ok(sse_event(&synthetic_sync(
                    &session_id,
                    last_seq,
                    &format!("replay failed: {error}"),
                )));
                return;
            }
        }
        let mut heartbeat = tokio::time::interval(Duration::from_millis(service.config().sse_heartbeat_ms));
        loop {
            tokio::select! {
                event = subscriber.next_event() => {
                    let Some(event) = event else { break };
                    if event.seq <= last_seq {
                        continue;
                    }
                    if event.seq > last_seq.saturating_add(1) {
                        let gap = event.seq.saturating_sub(last_seq);
                        let replay_window = u64::try_from(replay_limit).unwrap_or(u64::MAX);
                        if gap > replay_window {
                            yield Ok(sse_event(&synthetic_sync(
                                &session_id,
                                last_seq,
                                "live event gap exceeded the replay window",
                            )));
                            break;
                        }
                        let limit = i64::try_from(gap).unwrap_or(i64::MAX);
                        match store.list_event_frames(&session_id, last_seq, limit).await {
                            Ok(frames) => {
                                for frame in frames {
                                    if frame.seq <= last_seq {
                                        continue;
                                    }
                                    if frame.seq > event.seq {
                                        break;
                                    }
                                    last_seq = frame.seq;
                                    yield Ok(sse_frame(&frame));
                                }
                            }
                            Err(error) => {
                                yield Ok(sse_event(&synthetic_sync(
                                    &session_id,
                                    last_seq,
                                    &format!("gap replay failed: {error}"),
                                )));
                                break;
                            }
                        }
                        if event.seq <= last_seq {
                            continue;
                        }
                        if event.seq > last_seq.saturating_add(1) {
                            yield Ok(sse_event(&synthetic_sync(
                                &session_id,
                                last_seq,
                                "live event gap could not be fully replayed",
                            )));
                            break;
                        }
                    }
                    last_seq = event.seq;
                    yield Ok(sse_event(&event));
                }
                _ = heartbeat.tick() => {
                    // Pub/Sub is only a wake-up signal. Reconcile with the durable
                    // log so a lost terminal publish is repaired without a reconnect.
                    match store.get_session_summary(&session_id).await {
                        Ok(summary) if summary.last_event_seq > last_seq => {
                            let gap = summary.last_event_seq.saturating_sub(last_seq);
                            let replay_window = u64::try_from(replay_limit).unwrap_or(u64::MAX);
                            if gap > replay_window {
                                yield Ok(sse_event(&synthetic_sync(
                                    &session_id,
                                    last_seq,
                                    "reconcile gap exceeded the replay window",
                                )));
                                break;
                            }
                            let limit = i64::try_from(gap).unwrap_or(i64::MAX);
                            match store.list_event_frames(&session_id, last_seq, limit).await {
                                Ok(frames) => {
                                    for frame in frames {
                                        if frame.seq <= last_seq {
                                            continue;
                                        }
                                        last_seq = frame.seq;
                                        yield Ok(sse_frame(&frame));
                                    }
                                }
                                Err(error) => {
                                    yield Ok(sse_event(&synthetic_sync(
                                        &session_id,
                                        last_seq,
                                        &format!("reconcile replay failed: {error}"),
                                    )));
                                    break;
                                }
                            }
                        }
                        Ok(_) => {}
                        Err(error) => {
                            warn!(%error, %session_id, "session SSE reconcile failed; retrying on the next heartbeat");
                        }
                    }
                    yield Ok(Event::default().comment("heartbeat"));
                }
            }
        }
    }
}

fn sse_frame(frame: &SessionEventFrame) -> Event {
    Event::default()
        .event(frame.event_type.as_str())
        .json_data(&frame.event)
        .unwrap_or_else(|error| {
            Event::default()
                .event("error")
                .data(format!("serialization error: {error}"))
        })
}

fn synthetic_sync(session_id: &str, after_seq: u64, reason: &str) -> SessionEvent {
    let _ = session_id;
    SessionEvent::new(
        after_seq,
        "system",
        "system",
        None,
        SessionEventKind::SyncRequired {
            after_seq,
            reason: reason.to_string(),
        },
    )
}

fn sse_event(event: &SessionEvent) -> Event {
    Event::default()
        .event(event.event_name())
        .json_data(event)
        .unwrap_or_else(|error| {
            Event::default()
                .event("error")
                .data(format!("serialization error: {error}"))
        })
}

// ---------------------------------------------------------------------------
// Model/provider registry
// ---------------------------------------------------------------------------
