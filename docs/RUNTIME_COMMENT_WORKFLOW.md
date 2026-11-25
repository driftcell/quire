# RUNTIME COMMENT REVIEW WORKFLOW

## Metadata

- **Status:** Settled
- **Owner Crates:** `quire-protocol`, `quire-store`, `quire-server`, `quire-runtime`, `web`
- **Related Notes:** `../notes/runtime_comment_review_workflow.md`, `../notes/cross_block_annotation_ranges.md`, `../notes/structured_session_comment_refs.md`

## 1. Scope & Non-Goals

This document governs the session-workspace comment workflow: browser-to-runtime write proxying,
cross-block text/media selection, structured comment references in user messages, non-modal review UI, AI
handoff and addressed/archived transitions.

Comments remain revision-free side-channel data on `quire-server`. The canonical `CommentThread`
now carries multi-block anchors (`end_block_id`, ordered `anchor_blocks`) and optional start/end
quotes; this metadata never participates in Smart CAS or revision numbering.

## 2. Canonical Design & Contract

### Runtime comment proxy

The protected session router exposes:

- `POST /api/sessions/{session_id}/comments`
- `POST /api/sessions/{session_id}/comments/{thread_id}/replies`
- `PATCH /api/sessions/{session_id}/comments/{thread_id}`

Each handler resolves the session and requires a bound `doc_id` (or `linked_doc_id`). An unbound
session returns `400 INVALID_REQUEST` with a message explaining that a report must be linked first.
The handler then executes directly in-process against the embedded `PgReportStore` (`quire-store`)
with `actor = "reviewer"`:

- `report_store.create_comment_thread(&doc_id, ...)`
- `report_store.reply_comment_thread(&doc_id, &thread_id, ...)`
- `report_store.update_comment_status(&doc_id, &thread_id, status)`
- `report_store.delete_comment_thread(&doc_id, &thread_id)`

The resulting `CommentThread` JSON is returned directly and `CommentUpdatedEvent` is broadcast
through the shared `RedisStreamBus`. In addition, `quire_runtime::http::router` mounts
`quire_server::build_document_router` directly, so `/api/docs/{doc_id}/*` and `/mcp` are served by
the same backend process; `/api/sessions/*` remains runtime-owned.

The browser API is split by capability:

- `web/src/api/quire.ts`: read-only document/comment/diff reads.
- `web/src/api/runtime.ts`: session comment create/reply/status writes through the runtime proxy.

The standalone `/docs/{docId}` route remains read-only. It lists comments in a non-modal read-only
panel and never renders create/reply/archive controls.

### Cross-block anchors and detached detection

`CommentThread` keeps `block_id` as the starting/section anchor and adds:

- `end_block_id?: BlockId` for the final selected block.
- `anchor_blocks: Vec<BlockId>` in document order, including intermediate tables, images and charts.
- `end_quote?: TextQuoteSelector` for the end-block text fragment.
- `anchor_snapshot` may be a frontend-authored summary containing `[表格: …]`, `[图表: …]` or
  `[图片…]` placeholders.

Migration `0003_cross_block_comments.sql` adds `end_block_id` and `anchor_blocks`; migration
`0004_comment_end_quote.sql` adds `end_quote`. `anchor_detached` is true only when `block_id`,
`end_block_id` and every entry in `anchor_blocks` no longer exist. A block-ID filter matches any
anchor in the range, not just the start block.

### Structured comment reference protocol

`quire-session-protocol` defines `SessionCommentRef` as an immutable send-time snapshot with
`thread_id`, `block_id`, optional `end_block_id`, ordered `anchor_blocks`, `status`, optional start/end
quotes, optional `anchor_snapshot` and the review `comment`. `SendMessageRequest`, `SessionMessage`
and `PendingQueueItem` carry an ordered `comment_refs` array. `content` contains only the user's
natural-language instruction; it may be empty when references or attachments are present. Required
reference fields must be non-empty and a message cannot contain duplicate thread IDs.

```json
{
  "content": "Handle these comments together",
  "comment_refs": [
    {
      "thread_id": "ct_1",
      "block_id": "sec2_p3",
      "end_block_id": "chart_1",
      "anchor_blocks": ["sec2_p3", "tbl_1", "chart_1"],
      "status": "open",
      "quote": "selected source text",
      "end_quote": "selected end text",
      "anchor_snapshot": "selected source text … [表格: Metrics] … End text",
      "comment": "review comment body"
    }
  ]
}
```

Runtime migration `0006_session_comment_refs.sql` adds `comment_refs JSONB NOT NULL DEFAULT '[]'`
to `runtime.pending_queue`. Canonical conversation messages keep the same fields in their rig-memory
message-receipt Asides. Queue edits replace `content` and `comment_refs` together while preserving
attachments, source and the queue row's thread ID. Consecutive queued comment items still form one
user message: their references are flattened in FIFO order, non-empty content is joined with blank
lines and attachment IDs are deduplicated. Historical XML payloads are outside this clean contract
and are not parsed or migrated.

`quire-runtime/src/comment_refs.rs` is the single model-input projection. Ordinary messages remain
unchanged. Messages with references or attachments become one JSON object containing `instruction`
plus the non-empty `comment_refs` and/or `attachment_ids`, serialized with `serde_json`; the same
projection covers new turns, queued batches, retry and canonical compaction. A current prompt that
already ends canonical memory is supplied once without duplicating it in Runner history; automatic
compaction can exclude a newly appended prompt by its UI `message_id`. Comment-only, attachment-only
and combined messages therefore remain model-visible even with empty content.

### Composer and message stream

`PromptComposer` accepts `pendingCommentRefs`. Chips above the textarea show `#block_id` and a short
comment summary; clicking a chip removes that reference. On send, the composer passes the current
text, files and structured refs separately, then clears the refs only after a successful send. The
frontend never synthesizes fallback instructions for empty text: attachments render as attachment
cards, comments as reference cards, and only runtime adds metadata to the model prompt.

`SessionStreamView` reads `SessionMessage.comment_refs` directly. References become compact cards
showing block ID, status, optional quote and comment; clicking a card jumps to the report block.
`content` is rendered as the normal user bubble, and an empty body does not create an empty
paragraph. Pending messages use the same structured fields for editing, recall and deletion. Live
sends and history replay therefore render identically without an XML codec.

### Non-modal review workspace

The desktop workspace keeps the A4 report visible and splits the right panel between
`对话` and `批注 (N)` tabs. The comments panel is a full-height non-modal panel, not a Sheet; closing
it collapses the right panel. Mobile uses `报告预览 / Agent 对话 / 批注` tabs.

The panel supports `待处理 / 已处理 / 已归档` filters, replies, diff inspection and block jumps.
Every open card offers `交给 AI 修订`; the panel header offers `全部交给 AI 处理` for all open threads.
Both actions add `pendingCommentRefs` and switch to the chat tab, leaving the user free to add extra
instructions before sending.

When a thread is `addressed`, the card shows the latest agent reply and a `确认归档` action. Only this
human action archives a thread; agents move threads to addressed through `reply_comment` with
`mark_addressed=true`.

### Report highlights and cross-block activation

Selection capture runs inside `ReportViewer` and collects every visible `[data-block-id]` in document
order between the start and end blocks. A `Range` slice per boundary block produces exact text with
prefix/suffix context; intermediate tables, images and ECharts blocks contribute stable placeholders
to `anchor_snapshot`. Empty text selections still work when the intersecting blocks contain media.

Text highlights use the CSS Custom Highlight API (`CSS.highlights` + `::highlight()`):

- `quire-comment` for existing non-archived threads.
- `quire-comment-active` for the pending selection or the selected comment card.

No `<mark>` or wrapper elements are inserted into report content, so table colgroups and image
rendering are untouched. Non-text blocks receive a pseudo-element outline (`.quire-comment-scope`)
that does not affect layout. Browsers without CSS Highlights fall back to the same block-level
outline. Clicking a comment card scrolls to the starting block and activates both endpoint highlight
ranges plus the covered media outlines.

Images and chart canvases use `user-select: none` in report CSS and inside custom-HTML shadow roots,
preventing the native blue selection film while preserving text selection around media.

### Agent loop

The runtime sends the JSON envelope as the ordinary user message; agent definitions stay decoupled
from its wire shape. The lead agent follows the format-independent review workflow: read referenced
blocks, edit only the requested scope, then call `reply_comment(..., mark_addressed=true,
addressed_rev=N)` with a change summary. Agents never archive threads; humans confirm archive in the
panel.

## 3. Invariants & Verification

- The browser never receives or stores `QUIRE_RW_TOKEN`.
- Unbound sessions return a clear `400` instead of writing to an arbitrary document.
- Upstream proxy errors never include the runtime token.
- Structured references round-trip through REST, JSONB and session responses; duplicate thread IDs
  and empty required fields are rejected.
- Cross-block `anchor_blocks` are stored in document order; partial anchor survival keeps
  `anchor_detached=false` until every anchor is deleted.
- Text highlighting never mutates report DOM; tables and media keep their rendered structure.
- Media nodes remain `user-select: none`; text selection is still available around them.
- Live-sent and replayed messages use the same typed fields and card rendering.
- Comment-only, attachment-only and combined messages may have empty `content`; the frontend does
  not synthesize instructions, and runtime preserves all metadata in its JSON model projection.
- The comment panel is non-modal; the A4 report remains in the DOM while it is open.
- Only addressed threads expose `确认归档`; only humans archive.
- Verification: `cargo fmt --all -- --check`, `cargo clippy --workspace --all-targets -- -D warnings`,
  `cargo test --workspace`, plus `bun run test`, `bun run typecheck`, `bun run lint` and
  `bun run build` in `web`.
