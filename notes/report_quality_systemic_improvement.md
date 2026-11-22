# REPORT QUALITY SYSTEMIC IMPROVEMENT

## Metadata

- **Status:** Accepted
- **Date:** 2026-11-22
- **Promoted To:** `../docs/REPORT_LINT_RULES.md`, `../docs/REPORT_AUTHORING_QUALITY.md`

## 1. Context & Problem

Teacher review of a real 60k-character report and its complete main/subagent traces found good formal
compliance but weak publication quality. The report had 49 visual blocks and zero unsuppressed warnings,
yet contained multi-idea prose walls, editorial labels such as `核实值` and `（本报告建议）`, repeated
chart data, and unsupported post-review source additions.

The causes span several layers rather than one model prompt:

- the lead assigned hard per-chapter prose quotas and distributed sources/visuals by count;
- seven writers started in parallel without a pilot chapter or a full lead read-through;
- shared facts mixed verified values, unresolved conflicts and desired narrative;
- derived targets had no single owner, so arithmetic was patched across downstream blocks;
- reviewers emphasized arithmetic and regressions, while approval did not bind the final revision;
- analysts lacked source listing and table-preview tools, and reviewers lacked original-page fetching;
- `QR417` only fired at 450 visible characters, `QR419` could reward decorative visual insertion, and
  `QR103` pressured agents to attach source IDs even when a real block `source_note` was available.

## 2. Rejected Options & Why

- **Add broad semantic keyword bans for causality, metaphors or “AI writing”.** Legitimate uses are too
  common. Deterministic lint stays narrow; reviewers own semantic judgment.
- **Lower `QR417` to one aggressive character threshold.** A single cutoff penalizes concise multi-clause
  and legitimate long paragraphs alike. Length and sentence count provide a better wall-of-text signal.
- **Delete `QR419` or all of its exemptions.** Dense prose without visuals is a real defect, and
  recommendation sections legitimately favor prose. The repair hint must discourage decorative visuals
  instead of removing the rule.
- **Solve quality by requiring more charts, citations or prompt text.** Fixed counts caused the observed
  duplication. Rules and schemas should be disclosed progressively through diagnostics and guides, while
  agent prompts retain only durable editorial responsibilities.
- **Let every writer create visuals and derived targets.** Parallel ownership caused numbering, method and
  placement drift. Analysts own derived data and visuals; writers own prose and may request visual work.
- **Treat reviewer approval as timeless.** The observed run added sources after approval. Approval must
  name a revision and any later content mutation requires another review.

## 3. Chosen Approach & Trade-offs

Implement a layered contract:

1. **Lint:** add a precise editorial-meta-label rule; make `QR417` length-and-sentence aware; make
   `QR419` repair guidance prose-first and anti-decorative; let a non-empty block `source_note` satisfy
   `QR103` attribution without weakening citation identity checks.
2. **Tools:** add read-only `list_sources` with optional exact-ID filtering, and aggregate prose,
   paragraph, visual and cited-source counts in the document digest. Analysts receive report preview;
   reviewers receive source listing and read-only web search/fetch. After fresh-report acceptance,
   `QR419` moves from 1000 to 1500 visible characters because its old threshold fired on focused
   180–220-character paragraphs that had just been correctly split; `QR417` remains the wall-of-text
   guard while `QR419` catches only genuinely dense macro runs.
3. **Authoring workflow:** track explicit user requirements, distinguish verified/conflicting/hypothesis
   facts, pilot one chapter before broad parallel writing, centralize derived metrics and visuals, require
   lead section-by-section reading, and bind approval to a revision.
4. **Writing and review:** one paragraph has one main job; recommendations use direct language; visuals
   replace detail rather than duplicate it; chapter writers never conclude the whole report in parallel,
   and the lead writes or rewrites that conclusion only after the full read-through. Reviewers check
   readability, method comparability, citation support and post-approval cleanliness.

The trade-off is one additional MCP read tool and a pilot/review gate that adds latency before parallel
writing. In return, factual and stylistic defects are caught before they replicate across chapters, and
agents spend fewer turns guessing sources, counts and lint thresholds. Verification includes unit and
workflow tests plus a fresh same-prompt long-report generation inspected through its final document and
main/subagent traces.
