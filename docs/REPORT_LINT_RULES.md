# REPORT LINT RULES AND FILTERS

## Metadata

- **Status:** Settled
- **Owner Crates:** `quire-protocol`, `quire-lint`, `quire-store`, `quire-server`
- **Related Notes:** `../notes/quire_lint_registry_split.md`, `../notes/quire_lint_incremental_refactor.md`

## 1. Scope & Non-Goals

This document governs the static report linter: crate ownership, rule registration, diagnostic
contract, layered ignore/select behavior, persistence and MCP exposure.

It does not cover Markdown projection, ECharts rendering, frontend lint visualization or runtime
agent prompting. The linter is a pure synchronous library; it performs no database, HTTP, filesystem
or CLI work.

## 2. Canonical Design & Contract

### Crate ownership

| Crate | Responsibility |
| --- | --- |
| `quire-protocol` | Wire contracts: `Diagnostic`, `LintSummary`, `LintReport`, `LintConfig`, `Severity`; `ReportDocument.lint`; `Block.lint_ignore`; `BlockPatch.lint_ignore`; `PatchOp::SetLint`. |
| `quire-lint` | `RuleId`, `RuleMeta`, `RULES`, `LintOptions`, `lint_document`, `lint_document_with_options`, `lint_echarts_block` and the normalization pass. |
| `quire-store` | Persists `report_documents.lint` JSONB and block payload `lint_ignore`; runs lint for inline warnings and blocking errors. |
| `quire-server` | Exposes `lint_document(doc_id, select?, ignore?)` and forwards diagnostics to MCP receipts. |

`quire-lint` depends only on `quire-protocol`, `serde_json` and `regex`. There is no compatibility
module, re-export shim or legacy uppercase diagnostic code in `quire-protocol`.

### Diagnostic contract

Every diagnostic carries:

```json
{
  "code": "QR202",
  "rule": "table-empty-column",
  "severity": "warning",
  "message": "...",
  "hint": "...",
  "suppressible": true,
  "block_id": "financials",
  "path": "blocks[3].rows.spare"
}
```

`code` is the stable `QRxxx` identifier. `rule` is the kebab-case semantic name. `suppressible`
is `false` for core contract and safety rules and `true` for advisory rules. `LintSummary.suppressed`
counts suppressible diagnostics hidden by any ignore layer; it is omitted when zero.

### Rule registry

`quire-lint/src/registry.rs` defines `RuleId`, `RuleMeta`, `RULES` and `rule_matches_token`. Codes and
names are globally unique and code-sorted. The registry is the single source of truth; docs do not
duplicate individual rule metadata. Retired codes `QR002`, `QR008` and `QR405` remain absent and are
never reused. `QR002`/`QR008` were construction-time invariants already guaranteed by the validated
ID newtypes; `QR405` was subsumed by `QR411`'s manual-numbering diagnostic.

Category ranges:

| Range | Family | Policy |
| --- | --- | --- |
| `QR0xx` | Document/block/table core contract | Non-suppressible |
| `QR1xx` | Citations and source quality | `QR101` and `QR107` non-suppressible; quality advisories suppressible |
| `QR2xx` | Table layout risk | Suppressible warnings |
| `QR3xx` | ECharts safety/data | `QR301`-`QR304` non-suppressible; other chart checks suppressible |
| `QR4xx` | Structure and text quality | Suppressible except `QR424` internal-identifier safety |
| `QR5xx` | CSS/HTML/theme/cover/TOC | `QR501` non-suppressible; other style advisories suppressible |

### Source and table integrity

`QR108` parses each non-empty source URL, ignores case, port and a leading `www.`, and reports exact
or subdomain matches for the known wiki/self-media aggregator list (`baike.baidu.com`,
`baijiahao.baidu.com`, `toutiao.com`, `sohu.com`, `163.com`, `zhihu.com`, `csdn.net`, `jianshu.com`,
`360doc.com`, `bilibili.com`, `weibo.com`, `xiaohongshu.com`). Diagnostics are document-scoped and
do not flag lookalike domains such as `baike.baidu.com.evil.com`.

`QR103` recognizes numeric claims only when a number carries a quantitative unit or currency
marker. Higher-education units include `项`, `篇`, `所`, `名`, `位`, `倍`, `‰` and `百分点`; bare
years, durations, chapter numbers and structural counts such as `3个方面` do not trigger it.

`QR206` verifies `合计`/`总计` rows in canonical `summary_rows`, falling back to a final body summary
row when no canonical total exists. Body detail rows exclude a trailing total and at least two detail
rows are required. Non-negative fixed-point decimals and thousands commas are supported;
empty/`-`/`—` detail cells count as zero. Percentage, rate, rank, position and average columns are
not additive and are skipped. Comparison allows half a unit at the summary cell's displayed precision;
a note declaring `四舍五入` retains its existing whole-table exemption. Fixed-width 2-4 part integer
composites separated by `/`, `／` or `·` remain supported. Each mismatched summary cell reports with a
path into `rows` or `summary_rows`, and the hint carries the exact expected value.

`QR201` checks every explicit column width against the single-line header and a readable body floor;
numeric and summary cells retain their widest-cell requirement, and CJK entity text has a 76px
minimum. Explicit `column_widths` cannot exempt narrow columns. `QR204` reports any table with more
than nine leaf columns or a natural width above 1.18× the 640px A4 content width. Both hints direct
the writer to remove all-`—` columns, reduce to 6–8 columns (hard maximum 9), or split by topic
instead of hiding overflow with manual widths.

### Structure and placeholder integrity

`QR401` retains the original English and Chinese placeholder tokens and additionally detects `××`,
contextual `XX大学`/`XX学院`/`XX学科`/`XX项` forms, bracketed `【待…】`/`（待…）` slots, standalone
`N` followed by a quantifier, `?%` and `??`. Bare `某高校`, generic angle-bracket examples and
`待批复` remain valid prose rather than being promoted to placeholder errors.

`QR406` parses complete Chinese and English visual references. Valid global references must start at
one and resolve within the document's table/figure counts. Zero, out-of-range, chapter-scoped or
compound forms such as `表 3.2`, `图 2-1`, and `附表 1` report; the diagnostic retains the complete
reference and compound/appendix forms direct the writer to the renderer's global continuous labels.

`QR404` parses manual heading numbers into relative hierarchy components instead of binding `一、` to
H1. An unnumbered H1 therefore permits an H2-rooted `一、` → `（一）` → `1.` → `（1）` hierarchy;
Arabic documents may use `1.` → `1.1` → `1.1.1`. Chapter/section markers and multi-character Chinese
numerals such as `十一、` are supported. Sibling headings are grouped by actual parent block, level
and numbering family; duplicates, gaps, non-1 starts and changed numeric prefixes report within that
group. Decimal percentages such as `3.5%` remain prose rather than heading numbers.

### AI-slop prose checks

`QR412`-`QR415` analyze only `BlockKind::Paragraph`; headings are too short to judge and `Quote`
blocks are treated as external quotations. Before matching, the prose module masks inline code,
Markdown link targets, inline citations and `《...》` / `“...”` / `"..."` / `「...」` spans with spaces,
so policy titles and quoted proper nouns cannot become false positives. `QR412` requires one strong
cliché from process/disclaimer leakage or extreme internet jargon, or at least two distinct
empty-jargon words (with ecological compounds such as `生态环境` exempt).
Formal policy terms such as `高质量发展`, `内涵式发展` and `系统推进` are allowed. Evaluative phrases
such as `显著提升`, `稳中有进` and `提质增效` report only when their sentence lacks a number with a
quantitative unit; a bare year does not qualify. `组合拳` is allowed once but reports when densely
combined or repeated, while `与此同时` and `值得注意的是` report only on repeated use within one
paragraph or at the starts of consecutive paragraphs. `QR413` requires a long paragraph with no
digits, citations or book titles. `QR414` detects ordered mechanical connectives, the third and later
`一方面/另一方面` contrast pair, three or more empty bold-list headings, and punctuated
`其一/其二/其三` or `第一/第二/第三` enumerations. Three/four-item `一是/二是/三是[/四是]` runs are
allowed when the paragraph has quantitative or citation evidence, every item has at least eight
visible characters, and the items average at least eighteen; hollow or padded runs still report.
`QR415` counts sentence-bounded contrast pairs whose gap is 1-45 characters, plus strong hedging
phrases or two distinct light hedging phrases in one paragraph.

### Layout rhythm and chart readability

`QR316` protects ECharts label legibility. A pie/donut series with more than seven slices is dense;
on `xAxis` category labels, ten or more labels, or eight or more labels averaging at least six
visible characters, is dense unless `axisLabel.rotate` is non-zero or `axisLabel.interval` is
non-zero/`auto`. `yAxis` categories are intentionally exempt because horizontal bars own a row per
label.

`QR317` checks line-series `endLabel`: every enabled end label must set
`endLabel.textBorderWidth: 0` to avoid white-background double-stroke ghosting. Two or more line
series with end labels also need a top-level or series-level `labelLayout.hideOverlap: true` guard.

`QR318` targets each Cartesian value axis whose bound bar/line series contain at least two
non-negative integer values with a maximum in `1..=15`. The counterpart axis must be categorical, and
the value axis must set a positive integer `minInterval` or `interval`; otherwise ECharts may draw
fractional ticks such as `0.5` or `2.4`. Series are grouped by `xAxisIndex`/`yAxisIndex`, so dual-axis
and multi-grid charts are checked per axis and paths include the array index. Percentage axes are
exempt. Polar/radar and dual-numeric coordinates are skipped, as are dataset-driven, tuple-valued or
otherwise ambiguous series whose value dimension cannot be resolved safely.

`QR416` counts consecutive Table/ECharts/CustomHtml blocks. Empty text and headings do not reset the
streak; a non-empty Paragraph or Quote does. The third visual block in a streak reports once, so two
complementary visuals remain allowed.

`QR417` measures prose after replacing Markdown links with their anchor text and removing citations
and code markers. It only reports a single paragraph of 450+ visible characters without a
newline-delimited list. Cross-paragraph density is owned exclusively by `QR419`.

`QR418` only inspects headings at level 2 or deeper. It strips common manual numbering and
punctuation, then flags exact generic headings (`概述`, `行动建议`, `Overview`, etc.) and counting
shells such as `三大核心维度`. The first H2 may use `导论`/`引言`/`前言` and the last H2 may use
`结语`/`总结`; arbitrary middle or deeper generic headings still report. A counting heading with a
colon followed by at least six characters, or a long title with concrete data/entities, is exempt.
For other official-document columns, the repair hint preserves the column label and adds a concrete
judgment after a colon.

`QR419` detects dense prose sections that should become a visual. It tracks the current H2 section,
resets on a new H1/H2 and after any Table/ECharts/CustomHtml, and does not let H3-H6 split a prose run.
Four or more non-visual paragraphs (or non-empty quotes) totaling 1000+ visible characters in a
non-exempt section report once. Opening, conclusion and abstract sections are exempt, as are
recommendation, next-step, implementation and safeguard sections. Section matching uses the normalized
column label before any colon, so variants such as `四、结语` and `对策建议：……` receive the same
exemption.

`QR420` flags Table and ECharts notes whose trimmed start begins with `注：`, `注:`, `表注：`,
`表注:`, `图注：`, `图注:` or `Note:`/`note:`. The renderer already prepends `注：`, so keeping the
prefix produces `注：注：` in the PDF. CustomHtml notes are outside this rule.

### Higher-education writing checks

`QR421` analyzes paragraphs for blind ordinal references such as `第一项与第三项占优` and for
academic-method jargon including sample/variable terminology, regression/significance language,
Spearman/Pearson, HHI and p-value statements. An ordinal followed by an explicit metric name, such as
`第一项“国家级项目数”占优`, is allowed. Quoted titles and inline code are masked, and Spearman is
owned by this rule rather than duplicated in `QR412`.

`QR422` is a suppressible Warning covering three independent visual-publication checks. Numeric
Table/ECharts blocks with no non-empty `note` must have complete unit labels: every numeric table
column carries a unit in its header; charts may instead label every explicit value axis, or every
numeric series for charts without value axes. Units use forms such as `（项）`, `（万元）` and `（%）`.
A non-empty unit/scope/time note suppresses only this unit check. Data tables (body or summary rows)
and numeric charts must also have either `source_ids` or a non-empty `source_note`; the repair hint
requires both a natural-Chinese institution/database name and bound citations. Finally, any Table or
ECharts `note` longer than 180 Unicode scalar values reports separately, with a repair target of 120
characters (about two lines) and instructions to remove formulas, reading conclusions, name glossaries
and repeated missing-data explanations. Source and length findings still apply when a note is present.

`QR423` checks paragraphs, headings and Table/ECharts `title`/`note`/`source_note` for unquoted
`双一流`/`双优`/`十四五`/`十五五`, year ranges written with `-`, `~`, `～` or `–` instead of `—`, and
official document numbers using round rather than hexagonal year brackets. Existing Chinese quotation
marks and inline code are respected; Quote blocks are external material and are not rewritten by this
rule.

`QR424` is a non-suppressible Error covering every visible publication field: heading, paragraph and
quote text; Table titles, notes, source notes and every body/summary cell; ECharts titles, notes,
source notes and fallback summaries; CustomHtml titles, notes and fallback text. It detects ASCII
API/database assignments and year-primary-key mappings, `local:` metric codes, internal enum/field or
table names, `LATEST`, `学科云index系统`, panel terminology, and cell-level `学科云无数据` /
`双优库无数据`. Repairs must translate these into formal business Chinese or `—` / `暂未收录`, never
merely hide the rule.

### Matching and filter layers

`rule_matches_token` accepts:

- exact code: `QR202`
- exact semantic name: `table-empty-column`
- category prefix: `QR2`

Selection and suppression are applied in this order:

1. `LintOptions.select`: when non-empty, only matching rules are considered.
2. Call-level `LintOptions.ignore`.
3. Document-level `ReportDocument.lint.ignore`.
4. Block-level `Block.lint_ignore`, only for diagnostics anchored to that block.

A matching ignore token suppresses a diagnostic only when its `suppressible` flag is `true`.
Core contract and safety rules survive every ignore layer and are never counted in `suppressed`.

### Persistence and configuration

- Migration `crates/quire-store/migrations/0002_document_lint_config.sql` adds
  `report_documents.lint JSONB NOT NULL DEFAULT '{"ignore":[]}'`.
- `PatchOp::SetLint { lint }` replaces the document policy through the normal revision, inverse-op
  and rollback pipeline.
- `BlockPatch.lint_ignore` replaces the block list; an empty list clears it. Block data stays inside
  the existing `report_blocks.payload` JSONB.

### MCP behavior

`LintParams` accepts optional `select` and `ignore` arrays. `lint_document` forwards them to
`lint_document_with_options`; document and block filters still apply. A receipt with only suppressed
diagnostics remains `ok: true` but includes `summary.suppressed` so callers can audit what was hidden.

The protocol guide does not enumerate rule codes. It states the writing constraints and the generic
verify loop: fix every `error`, review `warning`, and ignore only diagnostics whose payload says
`suppressible: true` when the rule genuinely does not apply.

### Hint orchestration

`crates/quire-server/src/mcp/lint_hint.rs` turns diagnostics into one deterministic `_next_hint`.
Priority tiers are: errors, structural/chart/table/citation warnings, macro layout warnings
(`QR417`-`QR419`), prose/policy warnings (`QR412`-`QR415`, `QR421`, `QR423`), then info. Diagnostics are grouped by
`block_id` (or `document`), codes are deduplicated and tier-sorted, groups are ordered by tier,
diagnostic count and block ID, and only the first five groups expand with a `(+N more block(s))`
tail. A warning rule touching more than three blocks asks for the top five blocks to be fixed in one
`apply_operations` batch before `lint_document` is re-run for the remainder.

`apply_operations` now distinguishes three outcomes: blocking errors, inline warnings on edited
blocks, and a clean batch. The warning branch asks for one `apply_operations` batch before continuing
instead of blindly advancing the revision. `lint_document` uses the same grouped projection for its
error and warning hints. Individual rule hints follow a three-part shape: target field/block, minimal
repair action, and an anti-cheat guardrail (no synonym replacement; no invented numbers or sources).

## 3. Invariants & Verification

- `quire-lint` has no CLI and no I/O dependencies.
- Every `RuleMeta.code` and `.name` is unique; the registry stays code-sorted.
- `QR001`, `QR003`-`QR007`, `QR101`, `QR107`, `QR301`-`QR304`, `QR424` and `QR501` are never
  suppressible.
- Prefix, name and code matching produce the same rule set as the registry.
- `summary.suppressed` counts each suppressible diagnostic hidden once.
- Prose checks skip headings and quotations, mask quoted proper nouns before matching, and never
  report a single B-class jargon word as `QR412`.
- `QR421` exempts explicitly named metrics and owns statistical jargon without duplicating `QR412`;
  `QR422` is Warning and checks units, missing visual sources and notes over 180 characters independently;
  `QR423` respects Chinese quotes, inline code and external Quote blocks. `QR424` covers every visible
  publication field and survives document/block ignore policies while leaving natural business Chinese
  untouched.
- Contrast tics are sentence-bounded and require a 1-45 character gap; a single contrast sentence or
  a single light hedging phrase does not report `QR415`.
- Dense chart labels report at most once per chart; line end-label checks fire at most once per chart;
  `QR318` checks each eligible numeric axis independently with 2+ non-negative integer values and max
  `1..=15`; `QR404` evaluates numbering relative to the document root and checks only siblings in the
  same parent/level/family group; `QR416` reports only the third visual in a run; `QR417` checks only
  single-paragraph walls and never
  shares a multi-paragraph diagnostic with `QR419`; `QR418` skips level-1 headings, numbering-only
  prefixes and positional introduction/conclusion headings; `QR419` reports once per non-exempt H2
  prose run; `QR420` only inspects Table/ECharts notes.
- `QR108` matches hosts exactly or as subdomains, so lookalike domains do not report;
  `QR206` reports at most once per mismatched summary cell and carries the expected sum in its hint.
- Hint groups are deterministic, merged by block, capped at five expanded groups, and preserve tier
  order; `apply_operations` never advances past inline warnings without naming the affected blocks.
- Document and block ignore policies survive reload, revision updates and rollback.
- The full gate passes:

```bash
cargo fmt --all -- --check
cargo clippy --workspace --all-targets -- -D warnings
cargo test --workspace
cd web && bun test && bun run typecheck && bun run lint && bun run build
```

- `web/e2e/lint-document-workflow.js` verifies multi-rule detection, diagnostic metadata,
  block-level suppression, non-suppressible guardrails, `select`/`ignore`, `ok: true` after fixes
  and browser rendering with zero console errors.
