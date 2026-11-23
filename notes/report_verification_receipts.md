# Report verification receipts

## Metadata

- **Status:** Accepted
- **Date:** 2026-11-23
- **Promoted To:** `../docs/REPORT_AUTHORING_QUALITY.md`, `../docs/REPORT_LIVE_SYNC.md`

## 1. Context & Problem

Parallel authors advanced the document between a preview request and its screenshot. Reviewers also
received a raw Markdown character count without the agreed length, and a partial heading silently
selected an earlier subsection of the report.

## 2. Rejected Options & Why

- Marking a live screenshot with the requested revision cannot identify its contents. Use the store's
  existing revision reconstruction instead of keeping another preview history.
- Passing the length target only in reviewer prompts loses the agreement on hand-off. Storing it in
  cover metadata would publish editorial instructions. A fixed percentage tolerance would invent a
  requirement the user never chose.
- Selecting the first substring (or always preferring H1) guesses which section the caller intended.

## 3. Chosen Approach & Trade-offs

Pin preview reads to a committed revision and reject unverifiable render receipts. Persist an optional
document-wide character target at creation; report the actual readable paragraph text and its signed
difference in every digest. Old reports can be checked against an explicit target without a write.
Use exact heading IDs or unique text matches and return candidate IDs on ambiguity. Cross-block
consistency remains an editorial judgment verified from the updated blocks.
