# PENDING USER MESSAGE TRACE TIMELINE

## Metadata

- **Status:** Accepted
- **Date:** 2026-11-20
- **Promoted To:** `../docs/RUNTIME_RIG_STREAMING.md`

## 1. Context & Problem

A pending user message consumed between tool execution and the next model round is persisted and
appears in the outer session message list, but it has no corresponding run-trace segment. Live and
historical timelines can therefore place it before the tool activity that preceded its injection, and
subsequent thinking or tools remain folded into the preceding activity group. The event stream and
refreshed trace need one deterministic order: previous activity, injected user message, new activity.

## 2. Rejected Options & Why

- **Reorder only the frontend message list.** Rejected because DOM ordering cannot repair missing
  protocol information, live/history divergence or activity segmentation, and a refresh would lose
  the correction.
- **Treat every user message as inline run activity.** Rejected because the first user message of a
  normal turn belongs to the outer transcript, not to the run it started. Inline injection needs a
  distinct event and exact message-ID-based deduplication.
- **Add a separate mutable trace table.** Rejected because persisted `session_events` already provide
  ordered trace aggregation. A second trace write path would create consistency and transaction
  concerns without adding information.
- **Associate messages by run ID alone in the frontend.** Rejected because run-level hiding is too
  broad; deduplication must use the actual inline `UserMessage` segment IDs.

## 3. Chosen Approach & Trade-offs

Add `RunTraceSegment::UserMessage { message_id, content, created_at }` and a distinct
`user_message_injected` session event. Consuming a batch inside the rig tool loop stores the user
message with the current run ID and appends this event after the tool results and before the next
model request. Normal turn-start queue draining keeps `user_message_added` and does not set the
message run ID.

The frontend appends the same user segment to `liveMain`, closes the preceding activity, and starts a
new activity for later thinking/execution/text. Historical trace aggregation emits the identical
order. Rendering uses a shared user-bubble component with full message metadata when available, and
outer transcript rendering skips only message IDs known to be inline.

The accepted costs are a new wire event and additional message/trace metadata propagation. Trace
persistence remains event-based; no second trace store or migration is needed beyond the current run
ID column already present on `agent_messages`.
