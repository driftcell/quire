# RUNTIME ASK_USER SUSPENSION

## Metadata

- **Status:** Accepted
- **Date:** 2026-11-18
- **Promoted To:** `../docs/RUNTIME_ASK_USER.md`

## 1. Context & Problem

The agent needs to pause and ask the user structured questions. A blocking tool that waits on an
HTTP connection would consume a run task, break on runtime restart and complicate SSE. The runtime
instead needs a durable suspension: persist the card, mark the run awaiting input, close the current
execution, and resume when an answer arrives or the user bypasses the card.

## 2. Rejected Options & Why

- **Block the `ask_user` tool future until the user answers.** Rejected because it holds a runtime
  task and stream open indefinitely, loses the card on restart, and cannot survive rolling deploys.
- **Reuse the suspended run ID for the continuation.** Rejected because the suspended run already
  emitted its question terminal frame and its execution task is gone. A new run keeps the run/event
  envelope accurate and lets the old run be finalized as completed.
- **Store answers only in the next prompt without persisting context.** Rejected because later turns
  would lose the decision. A tool-role context message keeps answers in session history while
  remaining non-user-facing in the normal chat flow.
- **Add freeform flags to each question.** Rejected by protocol direction: the frontend always
  renders a fixed freeform/skip affordance, so `freeform`/`allow_freeform` would be duplicate,
  inconsistent policy.
- **Keep questions only in memory.** Rejected because `SessionDetail.pending_question` must restore
  cards after refresh and restart.

## 3. Chosen Approach & Trade-offs

`agent_questions` stores the card, its run and optional tool-call ID. The rig provider intercepts an
`ask_user` model call, emits the questionnaire request, and ends the execution loop. The service
persists the card, emits `questionnaire_requested`, marks the run `awaiting_input` and returns; no
long-lived connection is held.

Answering validates pending state, stores structured answers or the skip marker, emits
`questionnaire_answered`, writes a tool-role context message, finalizes the suspended run and starts
a fresh continuation run. A normal new message while a card is pending dismisses every pending card
with `superseded_by_user_message` and starts a new turn without being blocked by `awaiting_input`.

The costs are one extra run per answered card, a persisted tool-role context message and an
explicit no-freeform protocol (rendering policy lives in the frontend).
