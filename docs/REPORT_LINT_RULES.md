# REPORT LINT RULES AND FILTERS

## Metadata

- **Status:** Settled
- **Owner Crates:** `quire-protocol`, `quire-lint`, `quire-store`, `quire-server`
- **Related Notes:** `../notes/quire_lint_registry_split.md`

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

`quire-lint` depends only on `quire-protocol` and `serde_json`. There is no compatibility module,
re-export shim or legacy uppercase diagnostic code in `quire-protocol`.

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
duplicate individual rule metadata.

Category ranges:

| Range | Family | Policy |
| --- | --- | --- |
| `QR0xx` | Document/block/table core contract | Non-suppressible |
| `QR1xx` | Citations and source quality | `QR101` and `QR107` non-suppressible; quality advisories suppressible |
| `QR2xx` | Table layout risk | Suppressible warnings |
| `QR3xx` | ECharts safety/data | `QR301`-`QR304` non-suppressible; other chart checks suppressible |
| `QR4xx` | Structure and text quality | Suppressible, including placeholder errors |
| `QR5xx` | CSS/HTML/theme/cover/TOC | `QR501` non-suppressible; other style advisories suppressible |

### Source and table integrity

`QR108` parses each non-empty source URL, ignores case, port and a leading `www.`, and reports exact
or subdomain matches for the known wiki/self-media aggregator list (`baike.baidu.com`,
`baijiahao.baidu.com`, `toutiao.com`, `sohu.com`, `163.com`, `zhihu.com`, `csdn.net`, `jianshu.com`,
`360doc.com`, `bilibili.com`, `weibo.com`, `xiaohongshu.com`). Diagnostics are document-scoped and
do not flag lookalike domains such as `baike.baidu.com.evil.com`.

`QR206` verifies a final `合计`/`总计` row when the table has at least three rows and its note does
not declare `四舍五入`. Every non-first column is checked as a non-negative integer sum (thousands
commas allowed; empty/`-`/`—` count as zero in data rows) or as a fixed-width 2-4 part composite
separated by `/`, `／` or `·`. Each mismatched column reports once, and the hint carries the exact
expected value so the model can copy it without mental arithmetic.

### AI-slop prose checks

`QR412`-`QR415` analyze only `BlockKind::Paragraph`; headings are too short to judge and `Quote`
blocks are treated as external quotations. Before matching, the prose module masks inline code,
Markdown link targets, inline citations and `《...》` / `“...”` / `"..."` / `「...」` spans with spaces,
so policy titles and quoted proper nouns cannot become false positives. `QR412` requires one strong
cliché from mechanical transitions, process/disclaimer leakage, extreme internet jargon or
untranslated statistical names, or at least two distinct empty-jargon words (with ecological
compounds such as `生态环境` exempt); `QR413` requires a long paragraph with no digits, citations or book titles;
`QR414` detects ordered mechanical connectives, the third and later `一方面/另一方面` contrast pair,
three or more empty bold-list headings, and punctuated `其一/其二/其三` or `第一/第二/第三`
enumerations. A four-item `一是/二是/三是/四是` run reports regardless of digits or citations unless
the paragraph is a newline-delimited list. `QR415` counts sentence-bounded contrast pairs whose gap is
1-45 characters, plus strong hedging phrases or two distinct light hedging phrases in one paragraph.

### Layout rhythm and chart readability

`QR316` protects ECharts label legibility. A pie/donut series with more than seven slices is dense;
on `xAxis` category labels, ten or more labels, or eight or more labels averaging at least six
visible characters, is dense unless `axisLabel.rotate` is non-zero or `axisLabel.interval` is
non-zero/`auto`. `yAxis` categories are intentionally exempt because horizontal bars own a row per
label.

`QR317` checks line-series `endLabel`: every enabled end label must set
`endLabel.textBorderWidth: 0` to avoid white-background double-stroke ghosting. Two or more line
series with end labels also need a top-level or series-level `labelLayout.hideOverlap: true` guard.

`QR318` targets Cartesian bar/line charts whose series contain at least two non-negative integer
values with a maximum in `1..=15`. When exactly one axis is categorical, the numeric axis must set a
positive integer `minInterval` or `interval`; otherwise ECharts may draw fractional ticks such as
`0.5` or `2.4`. The check reports once and points at `option.xAxis` or `option.yAxis`. Polar/radar
charts, mixed series types and dual numeric axes are exempt.

`QR416` counts consecutive Table/ECharts/CustomHtml blocks. Empty text and headings do not reset the
streak; a non-empty Paragraph or Quote does. The third visual block in a streak reports once, so two
complementary visuals remain allowed.

`QR417` measures prose after replacing Markdown links with their anchor text and removing citations
and code markers. A paragraph of 450+ visible characters without a newline-delimited list is a wall
of text; five or more consecutive non-list paragraphs totaling 1000+ characters also report at the
threshold. Headings, tables, charts, custom HTML, quotes and list paragraphs reset the run.

`QR418` only inspects headings at level 2 or deeper. It strips common manual numbering and
punctuation, then flags exact generic headings (`概述`, `行动建议`, `Overview`, etc.) and counting
shells such as `三大核心维度`. A counting heading with a colon followed by at least six characters,
or a long title with concrete data/entities, is exempt.

`QR419` detects dense prose sections that should become a visual. It tracks the current H2 section,
resets on a new H1/H2 and after any Table/ECharts/CustomHtml, and does not let H3-H6 split a prose run.
Four or more non-visual paragraphs (or non-empty quotes) totaling 1000+ visible characters in a
non-exempt section report once. `导论`、`引言`、`前言`、`结语`、`总结`、`摘要` sections are exempt.

`QR420` flags Table and ECharts notes whose trimmed start begins with `注：`, `注:`, `表注：`,
`表注:`, `图注：`, `图注:` or `Note:`/`note:`. The renderer already prepends `注：`, so keeping the
prefix produces `注：注：` in the PDF. CustomHtml notes are outside this rule.

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
(`QR417`/`QR418`), prose warnings (`QR412`-`QR415`), then info. Diagnostics are grouped by
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
- `QR001`-`QR008`, `QR101`, `QR107`, `QR301`-`QR304` and `QR501` are never suppressible.
- Prefix, name and code matching produce the same rule set as the registry.
- `summary.suppressed` counts each suppressible diagnostic hidden once.
- Prose checks skip headings and quotations, mask quoted proper nouns before matching, and never
  report a single B-class jargon word as `QR412`.
- Contrast tics are sentence-bounded and require a 1-45 character gap; a single contrast sentence or
  a single light hedging phrase does not report `QR415`.
- Dense chart labels report at most once per chart; line end-label checks fire at most once per chart;
  `QR318` only fires on a single numeric axis with 2+ non-negative integer values and max `1..=15`;
  `QR416` reports only the third visual in a run; `QR417` never double-reports a block that already
  breached the single-paragraph wall limit; `QR418` skips level-1 headings and numbering-only
  prefixes; `QR419` reports once per non-exempt H2 prose run; `QR420` only inspects Table/ECharts
  notes.
- `QR108` matches hosts exactly or as subdomains, so lookalike domains do not report;
  `QR206` reports at most once per mismatched column and carries the expected sum in its hint.
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
