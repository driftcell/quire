# RUNTIME COMMENT REVIEW WORKFLOW

## Metadata

- **Status:** Settled
- **Owner Crates:** `quire-protocol`, `quire-store`, `quire-server`, `quire-runtime`, `web`
- **Related Notes:** `../notes/runtime_comment_review_workflow.md`, `../notes/cross_block_annotation_ranges.md`

## 1. Scope & Non-Goals

This document governs the session-workspace comment workflow: browser-to-runtime write proxying,
cross-block text/media selection, XML comment references in user messages, non-modal review UI, AI
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
The handler then forwards to `quire-server`:

- `/api/docs/{doc_id}/comments`
- `/api/docs/{doc_id}/comments/{thread_id}/replies`
- `/api/docs/{doc_id}/comments/{thread_id}`

The runtime uses `QUIRE_RW_TOKEN` from its own configuration and sends `x-session-id` plus
`x-actor: reviewer`. Upstream `CommentThread` JSON is returned unchanged. Upstream 4xx statuses are
preserved; network/5xx failures become `502` proxy errors. Tokens never appear in URLs, logs, error
messages or response bodies.

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

### XML comment reference protocol

Messages that hand comments to the agent start with one or more blocks:

```xml
<quire_comment_ref thread_id="ct_..." block_id="sec2_p3" end_block_id="chart_1"
                   anchor_blocks="sec2_p3,tbl_1,chart_1" status="open">
  <quote>selected source text (optional)</quote>
  <end_quote>selected end text (optional)</end_quote>
  <anchor_snapshot>selected source text … [表格: Metrics] … End text</anchor_snapshot>
  <comment>review comment body</comment>
</quire_comment_ref>
```

Single-block references omit the optional attributes/elements and remain fully supported.
`serializeCommentRefs(refs, userText)` escapes `&`, `<`, `>`, `"` and `'`, places all reference
blocks first, and appends the user instruction below. `parseCommentRefs(content)` extracts every
valid block, unescapes text, and returns the remaining body. If any block is malformed (missing
required attribute, comment, closing tag or quote close), the parser returns the original message
with no refs and never throws.

The XML is stored inside the normal user message content. No database schema change is required, so
freshly sent and replayed messages follow the same rendering path.

### Composer and message stream

`PromptComposer` accepts `pendingCommentRefs`. Chips above the textarea show `#block_id` and a short
comment summary; clicking a chip removes that reference. On send, the composer serializes the refs
with the current text and any quoted snippet, then clears the refs only after a successful send.

`SessionStreamView` parses user messages before rendering. Reference blocks become compact cards
showing block ID, status, optional quote and comment; clicking a card jumps to the report block. The
remaining body is rendered as the normal user bubble, and an empty body does not create an empty
paragraph. Live sends and history replay therefore render identically.

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

`agents/main.md` instructs the lead agent to parse `<quire_comment_ref>` blocks, read the referenced
block, edit only that scope, then call `reply_comment(..., mark_addressed=true, addressed_rev=N)`
with a change summary. Agents never archive threads; humans confirm archive in the panel.

## 3. Invariants & Verification

- The browser never receives or stores `QUIRE_RW_TOKEN`.
- Unbound sessions return a clear `400` instead of writing to an arbitrary document.
- Upstream proxy errors never include the runtime token.
- Valid XML references round-trip through serialize/parse, including `<>&"'`, multi-block
  attributes, end quotes and snapshots; malformed XML degrades to plain text without throwing.
- Cross-block `anchor_blocks` are stored in document order; partial anchor survival keeps
  `anchor_detached=false` until every anchor is deleted.
- Text highlighting never mutates report DOM; tables and media keep their rendered structure.
- Media nodes remain `user-select: none`; text selection is still available around them.
- Live-sent and replayed messages use the same parser and card rendering.
- The comment panel is non-modal; the A4 report remains in the DOM while it is open.
- Only addressed threads expose `确认归档`; only humans archive.
- Verification: `cargo fmt --all -- --check`, `cargo clippy --workspace --all-targets -- -D warnings`,
  `cargo test --workspace`, plus `bun run test`, `bun run typecheck`, `bun run lint` and
  `bun run build` in `web`.
