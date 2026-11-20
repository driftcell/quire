# STRUCTURED SESSION COMMENT REFERENCES

## Metadata

- **Status:** Accepted
- **Date:** 2026-11-20
- **Promoted To:** `../docs/RUNTIME_COMMENT_WORKFLOW.md`

## 1. Context & Problem

Comment references were embedded as XML in user message content. This mixed machine metadata with
natural language, forced the browser to maintain a custom XML codec, and made persistence, queue
editing, history rendering and model-input construction share one ambiguous string. The replacement
must preserve one-message/one-turn semantics, ordered queued-comment batches, immutable send-time
reference snapshots and the existing review workflow without requiring legacy-data migration.

## 2. Rejected Options & Why

- **Keep XML in message content.** Rejected because the browser, runtime and agent all need parallel
  parsing/serialization rules, malformed markup has ambiguous recovery behavior, and structured UI
  rendering otherwise requires reparsing persisted text.
- **Send references and instructions as separate user messages.** Rejected because turn ordering,
  atomicity, idempotency, queue recall and retry become more complex than with one message envelope.
- **Structure only the browser-to-runtime request.** Rejected because storing projected prompt text
  would still lose typed references in history, queue editing and frontend rendering. The structured
  representation must span the wire contract, persistence and session responses.
- **Preserve an XML compatibility path.** Rejected for this clean implementation because dual read or
  dual write paths preserve the ambiguity and substantially increase the test matrix. Historical XML
  migration is explicitly out of scope.

## 3. Chosen Approach & Trade-offs

Use `SessionCommentRef` values in `SendMessageRequest`, `SessionMessage` and `PendingQueueItem`.
Store `comment_refs` as JSONB beside message content in both runtime message tables. The browser
renders cards directly from typed references and sends natural-language `content` separately.

A single pure runtime projection converts content, references and attachment IDs into the
model-visible user input. Messages without metadata remain byte-for-byte unchanged; messages with
references or attachments use a JSON user envelope generated with `serde_json`. The same projection
covers new turns, queued batches, retry, history and compaction. Comment-only and attachment-only
messages may have empty content. The frontend never synthesizes fallback instructions for those cases.

The accepted costs are a runtime schema migration, additional JSONB payloads and one model-prompt
format change. Legacy XML messages are intentionally not migrated or interpreted by the new path.
