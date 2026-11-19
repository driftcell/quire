# RUNTIME COMMENT REVIEW WORKFLOW

## Metadata

- **Status:** Accepted
- **Date:** 2026-11-19
- **Promoted To:** `../docs/RUNTIME_COMMENT_WORKFLOW.md`

## 1. Context & Problem

The browser SPA only receives the read-only Quire token. It could list comments but could not create,
reply or archive them without the `rw_token`, which must never be exposed to the client. Separately,
the modal comment drawer covered the A4 report, making visual review awkward, and there was no compact
way to hand a specific comment to the agent without retyping its context.

## 2. Rejected Options & Why

- **Ship `QUIRE_RW_TOKEN` to the browser.** Rejected because any script on the page could then mutate
  every report. The runtime already owns the server credential and must remain the only writer.
- **Persist XML comment references in a new database table.** Rejected because the reference cards are
  a message-rendering concern, must survive history replay, and do not need revision or query
  semantics. The XML travels inside the user message content and is parsed on read.
- **Keep the modal Sheet and add a second floating list.** Rejected because the overlay hides the A4
  body and duplicates list/filter/state logic between the drawer and popover.
- **Use free-form prose instead of a structured reference.** Rejected because block IDs, thread IDs
  and quotes would be ambiguous to the agent and impossible to render consistently after refresh.
- **Let the browser send a full batch of comments and instructions as separate messages.** Rejected
  because ordering and idempotency become fragile; one serialized message keeps the XML header at the
  top and the user instruction below it.

## 3. Chosen Approach & Trade-offs

`quire-runtime` exposes three session-scoped comment proxy endpoints. Each resolves the session's
bound `doc_id`, rejects unbound sessions, attaches the runtime-owned `QUIRE_RW_TOKEN`, and forwards to
the matching `quire-server` comments endpoint. Comment data and UI state never depend on a browser
write token.

The frontend serializes selected threads as `<quire_comment_ref>` XML blocks followed by the user's
instruction. The parser strips all valid blocks into compact cards, preserves the remaining body for
Markdown rendering, and degrades malformed XML to ordinary text. The same parser runs for freshly sent
messages and history replay.

The right workspace panel is a non-modal `对话 / 批注 (N)` tab split. The floating popover is reduced
to a one-shot "create comment at this anchor" form. Comment cards can be handed to the agent, then
reply/archive after the agent marks a thread addressed. Quote highlights in the report open and
activate the matching comment card without covering the report.

Accepted costs: message bodies carry a small XML header, the reference status is a snapshot from send
time, and the runtime must have `QUIRE_RW_TOKEN` configured for any comment write.
