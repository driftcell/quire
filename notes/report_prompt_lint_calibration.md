# REPORT PROMPT AND LINT CALIBRATION

## Metadata

- **Status:** Accepted
- **Date:** 2026-11-21
- **Promoted To:** `../docs/REPORT_LINT_RULES.md`

## 1. Context & Problem

Five end-to-end higher-education reports were generated from short, ordinary user requests:
“双一流” benchmarking, “十五五” discipline planning, high-level talent teams, “双优” industry
alignment, and degree-authorization layout. All five reached zero unsuppressed lint warnings and
passed browser print-overflow checks, yet teacher review still found factual contradictions,
unsupported causal claims, stale suppressions and low-value visuals added only to satisfy lint.

The calibration problem is therefore not “more rules” versus “fewer rules”. Deterministic layout and
arithmetic checks should remain strict, while prompts must not force decorative numbers, duplicate
sources or redundant visuals. Human/model review remains responsible for semantic validity.

## 2. Rejected Options & Why

- **Require every H1 to contain an Arabic number.** This produced resistance in ordinary generation
  and would reward artificial titles. Evidence chapters should prefer quantified judgments; action
  and conclusion headings may stay qualitative when the judgment is already concrete.
- **Require both `source_note` and `source_ids` on every visual.** The renderer can resolve readable
  publisher/title names from registered bibliography entries, and recommendation-only tables may not
  have external data. The publication invariant is a readable, traceable source, not two populated
  fields at any cost.
- **Require a separate concluding H1 in every report.** Some reports already close effectively inside
  their final action chapter. A mandatory extra chapter repeats the summary. If a conclusion chapter
  exists it must be last, and no scope-only chapter may follow it.
- **Delete every constant column.** An all-zero entity column can be the central comparison evidence,
  such as a school having no recorded national awards. Such a column may remain with an explicit note
  and a local, justified suppression; repetitive labels such as an unchanged degree type should move
  to the title/note.
- **Suppress or remove QR205, QR206 and QR419 wholesale.** Each catches real defects. Their observed
  false positives came from specific structural blind spots and should be fixed narrowly instead.
- **Add keyword lint for causality and official-policy claims.** Phrases such as “决定”, “根源” and
  “先行指标” have legitimate uses. Static keyword bans would create false positives; reviewer prompts
  must require direct evidence and calibrated inference language instead.

## 3. Chosen Approach & Trade-offs

QR205 ignores explicitly labelled percentage indicator rows when entity columns also contain count
rows. QR206 recognizes direct/short-entity summary labels but not a metric's own parenthesized or
long-prefix time-window total. QR419 does not demand another visual in sections already served by two
visuals, and treats recommendation/action/path sections as prose-friendly at H1 as well as H2.

Prompts now distinguish facts, interpretation and recommendations. Cross-sectional stock ratios do
not prove transition speed or productivity; proxy denominators do not justify staffing decisions;
internal suitability scores are not official evaluations. Reviewers explicitly check ordered
“分别/依次” lists, absolute claims and stale `lint_ignore` entries against the current revision.

The accepted limitation is that semantic overclaiming cannot be eliminated by deterministic lint.
The project trades a smaller number of precise automatic warnings for a stronger final review pass,
and keeps justified local suppression available for genuinely meaningful exceptions.
