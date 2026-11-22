# REPORT AUTHORING QUALITY

## Metadata

- **Status:** Settled
- **Owner Crates:** `quire-server`, `quire-runtime`, `quire-lint`
- **Related Notes:** `../notes/report_quality_systemic_improvement.md`, `../notes/report_prompt_lint_calibration.md`

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
hard per-chapter prose quotas. `get_document_digest` exposes `paragraph_count`, `prose_char_count`,
`visual_block_count`, `source_count` and `cited_source_count` for requirement checks.

Shared facts have three states:

- **Verified:** value, unit/scope, time window and original source are known.
- **Conflict:** competing values remain side by side without an invented explanation; downstream prose
  must not use them as settled facts.
- **Hypothesis:** an interpretation to test, never part of the verified fact lock.

Search snippets are leads only. A citation must support its specific claim, and an unrelated source does
not count toward a user's source requirement. `list_sources(doc_id)` returns every registered source;
`list_sources(doc_id, source_ids=[...])` returns exact records and evidence excerpts in request order.
Unknown IDs fail with `INVALID_PARAMS` and the valid-ID list instead of disappearing silently.

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
   parallel. Writers own prose and headings only; they request visuals or derived calculations instead
   of creating them to satisfy lint.
5. The lead reads every chapter through `read_as_markdown(section_heading=...)`, checks requirements,
   repetition, methods, titles and readability, and asks analysts to consolidate visuals when needed.
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
| `writer` | Skill resources, digest, `read_as_markdown`, `read_blocks`, `list_sources`, `lint_document`, assigned internal data tools | `apply_operations` for assigned prose/headings | No source registration, visual creation/moves or invented derived targets; reports evidence and visual gaps |
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
- Source count, visual count, zero lint warnings and zero page overflow are necessary checks where
  applicable, but none independently proves publication quality.
- A reviewer cannot approve only the edited regression subset: final approval requires the full current
  document and all post-approval content changes to be reviewed again.

## 3. Invariants & Verification

- `list_sources` preserves request order for exact-ID queries, returns evidence excerpts and rejects
  unknown IDs with valid IDs in the error details.
- Digest aggregates describe the whole document even when `query` filters the returned block list.
- Native/server tool catalogs have matching 19-tool sets, and the multi-agent workflow E2E covers
  `list_sources`, digest aggregates, Smart CAS and review tools.
- Lint tests cover editorial labels, length-and-sentence walls, source-note attribution and the
  anti-decorative `QR419` repair contract.
- The authoring workflow is verified through unit tests, `quire-server/tests/e2e_multi_agent_workflow.rs`,
  runtime native-tool tests and fresh same-prompt report generation with document and trace review.

Full gate:

```bash
cargo fmt --all -- --check
cargo clippy --workspace --all-targets -- -D warnings
cargo test --workspace
cd web && bun test && bun run typecheck && bun run lint && bun run build
```
