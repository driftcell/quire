# REPORT AUTHORING QUALITY

## Metadata

- **Status:** Settled
- **Owner Crates:** `quire-server`, `quire-runtime`, `quire-lint`
- **Related Notes:** `../notes/report_quality_systemic_improvement.md`, `../notes/report_prompt_lint_calibration.md`, `../notes/report_verification_receipts.md`

## 1. Scope & Non-Goals

This document governs the higher-education authoring workflow in `agents/` and
`skills/higher-education-writing/`, the report tools exposed to each agent role, and the evidence and
review contracts used for long multi-agent reports. Static diagnostic semantics live in
`REPORT_LINT_RULES.md`; native tool registration lives in `RUNTIME_MCP_REGISTRY.md`.

The workflow guides model behavior; it is not a database transaction or a general semantic evaluator.
Reviewer approval is a prompt-level, revision-bound editorial contract, not a separate persisted report
state. Factual reasoning, citation relevance and publication readability require model review rather
than keyword-only lint.

## 2. Canonical Design & Contract

### Requirements, evidence and derived data

The lead records explicit user requirements before authoring: total length, effective source count,
comparison set, time window, requested calculations and layout. Total length is never decomposed into
hard per-chapter prose quotas. When a report is created with an agreed total, `create_report` stores
it as `target_prose_chars`; `get_document_digest` then returns `target_prose_chars` and a signed
`prose_char_delta` (actual minus target) alongside `paragraph_count`, `prose_char_count`,
`visual_block_count`, `source_count` and `cited_source_count`. `prose_char_count` counts visible
paragraph text only: inline citations and Markdown markers are excluded so the number matches what a
reader sees. An under-target delivery must add substantive analysis or disclose the gap; clean lint
does not discharge the length agreement. Older reports without a stored target may pass
`target_prose_chars` explicitly for a read-only comparison.

Shared facts have three states:

- **Verified:** value, unit/scope, time window and original source are known.
- **Conflict:** competing values remain side by side without an invented explanation; downstream prose
  must not use them as settled facts.
- **Hypothesis:** an interpretation to test, never part of the verified fact lock.

Search snippets are leads only. A citation must support its specific claim, and an unrelated source does
not count toward a user's source requirement. `list_sources(doc_id)` returns every registered source;
`list_sources(doc_id, source_ids=[...])` returns exact records and evidence excerpts in request order.
Unknown IDs fail with `INVALID_PARAMS` and the valid-ID list instead of disappearing silently.

### Tool receipts and merge semantics

`lint_document` returns `suppressed_blocks` (`block_id -> ignored codes`) whenever any block carries a
non-empty `lint_ignore`, even when there are no visible diagnostics. The final suppression audit reads
this map and clears stale entries instead of relying on a clean diagnostic list. A batch that only
writes Table/ECharts/CustomHtml blocks suppresses `QR416` in that immediate `apply_operations` receipt,
and a heading-only skeleton batch suppresses `QR403`: those conditions are transient until writers add
intervening prose or section bodies, while a full `lint_document` still reports them.

`upsert_sources` treats `title` and `url` as optional for an update to an existing `source_id`; a new
source still needs at least a non-empty title or URL. `patch_block` accepts patch fields either nested
under `patch` or flattened beside `block_id` (nested values win), which keeps model-produced hybrid
payloads self-healing. ECharts `option_patch` merges component arrays (`series`, `xAxis`, `yAxis`,
`grid`, `dataset`, `radar`, `polar`, `visualMap`, `dataZoom`, `graphic`) element by element, so an
agent can patch one series without resending the full option; switching an axis to `category` removes
stale numeric-axis keys (`min`, `max`, `interval`, and related fields) unless the patch supplies them.

The analyst is the single owner of derived values and visual blocks. Every target, ratio, difference,
slope and milestone records its baseline, time window, stock/flow/total/net-change semantics, formula,
reference entity, source and assumptions. A changed assumption requires recomputing every downstream
value rather than patching isolated prose numbers.

### Nine-step long-report workflow

1. Record hard requirements, collect original evidence and partition shared facts by state.
2. Build the elastic outline and assign each evidence set one primary chapter.
3. Have analysts verify high-risk facts, centralize derived data and build question-driven visuals with
   one shared entity palette/style contract.
4. Complete and calibrate one representative pilot chapter before starting the remaining writers in
   parallel. Writers own only their assigned body prose and headings; they never draft the whole-report
   conclusion early and request visuals or derived calculations instead of creating them to satisfy lint.
5. The lead reads every chapter through `read_as_markdown(section_id=<heading block ID>)` from the
   digest, checks requirements,
   repetition, methods, titles and readability, asks analysts to consolidate visuals when needed, and
   personally writes or rewrites the whole-report conclusion after every body chapter is complete.
6. An independent reviewer checks facts, original citations, derived methods, prose/visual redundancy,
   publication readability and actual A4 rendering, leaving anchored comments.
7. The lead resolves every comment or records a reason for not accepting it, recomputes downstream data
   when assumptions change, and finalizes all headings before approval.
8. The same reviewer resumes for continuity and returns `CHANGES_REQUESTED rev=N` or
   `APPROVED rev=N`. Approval covers the current text, visuals, citations and derived data only; any
   later change to those requires another review.
9. Apply layout-only changes after content approval. Recheck lint, hard requirements and actual
   rendering; return to independent review if content still needs changes.

### Role and tool boundaries

| Role | Required read/verify tools | Mutation tools | Deliberate boundary |
| --- | --- | --- | --- |
| `main` | Full native/tool catalog as needed, including `read_as_markdown`, `list_sources`, digest, preview and web evidence tools | Report orchestration, source registration, block patches and comments | Owns requirements, fact states, evidence ownership, full read, global edits and revision-bound delivery |
| `analyst` | `get_protocol_guide`, digest, `read_as_markdown`, `read_blocks`, `list_sources`, `preview_echarts`, `preview_report`, `deepseek_fetch`, internal data tools | `apply_operations`, `commit_chart`, `upsert_sources` | Sole owner of derived data, tables and charts; must inspect actual table rendering before layout suppression |
| `writer` | Skill resources, digest, `read_as_markdown`, `read_blocks`, `list_sources`, `lint_document`, assigned internal data tools | `apply_operations` for assigned prose/headings | No source registration, visual creation/moves, invented derived targets or premature whole-report conclusion; reports evidence and visual gaps |
| `reviewer` | `read_as_markdown`, `read_blocks`, `list_sources`, `lint_document`, `preview_report`, `deepseek_search`, `deepseek_fetch` | `add_comment`, `reply_comment` | Independent fact/method/source/readability/layout review; no direct report-content edits |
| `designer` | Digest, `list_sources`, `lint_document`, `preview_report` | Cover, TOC, references, page chrome and theme tools | Layout and bibliography presentation only; never repairs content by changing citation ownership |

`REPORT_TOOL_NAMES` and `BUILTIN_TOOLS` contain the same 19 report-domain tools. The server guide
progressively discloses source inspection, pilot ownership, revision-bound review and Smart CAS details;
agent prompts do not duplicate the complete lint catalog.

### Publication and review invariants

- One paragraph has one main job; evidence, mechanism and recommendation paragraphs are not forced into
  the same numeric template.
- A visual replaces detailed prose instead of licensing its repetition. Agents never insert duplicate
  visuals merely to reset a prose-run diagnostic.
- Recommendations use direct language; `核实值`, `属推断`, `本报告判断` and `本报告建议` are not used as
  editorial labels. Necessary method assumptions live in concise notes.
- The whole-report conclusion is written or rewritten by the lead only after every body chapter and the
  full read-through are complete; chapter writers may close their own section but never conclude the
  whole report in parallel.
- Source count, visual count, zero lint warnings and zero page overflow are necessary checks where
  applicable, but none independently proves publication quality.
- Final suppression review covers the complete block list, not only blocks edited in the latest round.
  Stale `lint_ignore` entries are removed; active entries require current-revision evidence.
- A reviewer cannot approve only the edited regression subset: final approval requires the full current
  document and all post-approval content changes to be reviewed again.

## 3. Invariants & Verification

- `read_as_markdown(section_id=...)` returns exactly the named heading and its contents. The legacy
  text selector fails closed with candidate IDs when a string matches multiple headings; it never
  guesses. `block_ids=[heading]` returns only the heading block, not the section.
- A preview of revision N either renders revision N or fails; a screenshot is never returned as
  evidence for a different revision. The same pinning applies to `GET /api/docs/{doc_id}?rev=N`.
- `prose_char_count` excludes citation markers and markup, and `prose_char_delta` is computed against
  the persisted `target_prose_chars` so every reviewer sees the same agreement.
- `list_sources` preserves request order for exact-ID queries, returns evidence excerpts and rejects
  unknown IDs with valid IDs in the error details.
- Digest aggregates describe the whole document even when `query` filters the returned block list.
- Native/server tool catalogs have matching 19-tool sets, and the multi-agent workflow E2E covers
  `list_sources`, digest aggregates, Smart CAS and review tools.
- Lint tests cover editorial labels, length-and-sentence walls, source-note attribution and the
  anti-decorative `QR419` repair contract.
- The authoring workflow is verified through unit tests, `quire-server/tests/e2e_multi_agent_workflow.rs`,
  runtime native-tool tests and fresh same-prompt report generation with document and trace review.

### Same-prompt acceptance evidence

A fresh session (`sess_y416xn07sh76`) used the original user prompt and the original scope answers. One
pilot writer completed before five remaining writers started in parallel. The final document
`2021-2025-6h89g98h` is at revision 69 and contains 297 blocks, including 261 paragraphs, seven tables
and four ECharts charts. All 30 registered sources have evidence excerpts and are cited.

After citation-marker cleanup, the prose contains 51,884 visible characters across 261 paragraphs
(mean 198.8, maximum 353). There are no `QR417` or `QR426` diagnostics and no editorial meta-labels.
The lead rewrote the conclusion after all body chapters were complete; it is the final level-1 section,
followed only by references. Browser checks confirmed all 297 blocks, the original prompt and final
approval message, polished conclusion wording, and 58 A4 pages with no horizontal overflow or console
errors.

Final live lint reports zero errors and zero suppressed diagnostics. Eighteen `QR419`, two `QR204`, four
`QR205` and eleven `QR413` diagnostics remain visible by design: the corresponding prose/visual and A4
reviews found no repair that would add information rather than duplication. A full block audit removed
83 stale `QR419` suppressions instead of hiding those diagnostics again. The reviewer returned
`APPROVED rev=69`, with no content or metadata mutation after approval.

The generation also exercised the pending-failure recovery path: a failed main run released its queued
message once no active run remained, the queue emptied, canonical user/assistant receipts projected
successfully, and the final run completed without another prefix error.

Full gate:

```bash
cargo fmt --all -- --check
cargo clippy --workspace --all-targets -- -D warnings
cargo test --workspace
cd web && bun test && bun run typecheck && bun run lint && bun run build
```

## 8. Trace-Driven Systemic Optimizations (`sess_y416xn07sh76` Forensic Calibration)

Forensic inspection of the 19 agent runs and 429 tool calls in session `sess_y416xn07sh76` (`doc_id=2021-2025-6h89g98h`, `rev=69`) identified ten systemic friction points across tool contracts, lint rules, preview routing, and agent prompts:

1. **Canonical Inline Citations in `read_as_markdown` (`crates/quire-protocol/src/markdown/`)**:
   - `MarkdownProjector` previously expanded inline `[^source_id]` into `[^source_id: Source Title]` inside projected paragraph text, causing writers and the lead agent to copy or mimic `[^id: title]` when patching blocks and triggering 20+ non-suppressible `QR107` blocking errors.
   - Inline citations now project as canonical `[^source_id]`, while resolved source titles are listed beneath each block in `Sources: [^id] (Title)`.
2. **Heading ID & Ordinal Chapter Resolution in `read_as_markdown` (`crates/quire-protocol/src/markdown/`)**:
   - `Selection::SectionAt` now matches exact Heading `block.id`s (`"ch1"`, `"c3-2"`), plain-text substrings, and Chinese/English ordinal chapter queries (`"第一章"`, `"第2章"`, `"第三章：师资与人才"`, `"Chapter 1"`), eliminating first-turn lookup errors in parallel writers. Canonical entry is now the `section_id` parameter; the legacy text selector fails closed when a string matches multiple headings.
3. **Partial `preview_report` TOC Suppression (`web/src/routes/docs.$docId_.preview.tsx`)**:
   - When `blockIds` or `sectionId` is set (`isPartial = true`), `filteredDocument` and `filteredComputed` now disable `toc` (`enabled: false`, `toc_tree: []`) alongside `cover` and `references`, so Page 1 directly renders the target table or chart rather than a full-page TOC.
4. **Atomic Source Attribution & In-Place Update in `preview_echarts` / `commit_chart` (`crates/quire-runtime/src/rig_agent/builtin_tools/`)**:
   - Both `preview_echarts` and `commit_chart` now accept optional `source_ids` and `source_note`, binding chart attribution in one transaction without a transient `QR422` warning.
   - When `commit_chart` targets an existing `Echarts` `block_id`, it now emits `PatchOp::PatchBlock` (and optional `MoveBlock` if a new anchor is supplied) instead of creating a colliding duplicate block.
5. **CJK Default TOC/References Titles & Cover Initialization (`crates/quire-server/src/mcp/handlers/`, `schema/`)**:
   - `exec_create_report` automatically sets `toc.title = "目录"` and `references.title = "参考文献"` whenever `title` contains CJK characters, and accepts optional `subtitle`, `authors`, `date`, and `cover_meta` fields at creation time.
6. **Lint Rule Precision (`crates/quire-lint/src/rules/{structure.rs,tables.rs,prose.rs,internal.rs}`)**:
   - `QR418`: Expanded counting adjectives (`结构性`, `系统性`, `阶段性`, `战略`) and counting nouns (`判断`, `主攻线`, `主线`, `路径`, `问题`, `短板`, `任务`, `行动`, `工程`) to catch bare counting headings like `三条结构性判断` and `四条主攻线`.
   - `QR205`: Exempted explicitly labelled ratio, percentage, average, and score rows (such as `领军人才数量省占比（%）`, `国家自然科学基金年均立项（项）`, `产业契合度（分）`) in row-oriented multi-indicator tables from false-positive decimal-spread warnings against integer count rows.
   - `QR419`: Preserved parent H1 action-path/recommendation/conclusion exemptions (`行动路径`, `对策建议`, `结语`) across child H2 subsections.
   - `QR424` & `QR426`: Added internal platform shorthand (`指标库口径`, `指标库记录`) to `QR424` and section-boundary self-narration (`本节只确认`, `本章只确认`, `在行动路径章统一处理`, `需要在申报口径上收敛的一处细节`, `这一位置决定了它在申报中的用法`, `是基于学科名称的推断`) to `QR426`.
7. **SOTA Multi-Agent Context & Prompt Architecture (`agents/*.md`, `skills/higher-education-writing/**`)**:
   - Stripped generic coding-assistant boilerplate from `agents/main.md` and added an explicit `<delegation_brief_contract>` that replaces per-section character count quotas with question-driven synthesis (3–4 analytical questions / 6–8 focused paragraphs per H2), mandates `定位词：具体判断` on both H1 and H2 headings, enforces verbatim `SourceEntry.excerpt` coverage, and requires mid-section visual interleaving (`after_block_id`).
