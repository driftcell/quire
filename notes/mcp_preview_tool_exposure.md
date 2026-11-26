# MCP PREVIEW TOOL EXPOSURE

## Metadata

- **Status:** Accepted
- **Date:** 2026-11-26
- **Promoted To:** `../docs/RUNTIME_CHART_PREVIEW.md`

## 1. Context & Problem

The report `/mcp` surface exposed 19 document tools, while visual verification was available only to
native Rig agents; `preview_report` also had an administrator-only adapter. External MCP authors could
not run the same report inspection or the `preview_echarts -> commit_chart` workflow. The implementation
must preserve the one-way runtime/server boundary, document/session ACLs and optional image payloads.

## 2. Rejected Options & Why

- **Expose a second preview-specific MCP endpoint.** Rejected because one authoring workflow would need
  multiple server connections and duplicated identity/discovery setup.
- **Move Rig tools into `quire-server` or call runtime internals directly.** Rejected because the server
  must remain independent of runtime and Rig.
- **Expose Playwright or sandbox execution directly.** Rejected because raw browser/code execution is a
  much larger security boundary than controlled report/chart previews.
- **Treat `preview_id` as the only authorization credential.** Rejected because external preview/commit
  calls need explicit user/session ownership, document ACLs and chat-session checks.
- **Always return PNG screenshots.** Rejected because base64 images are wasteful for text-only clients.

## 3. Chosen Approach & Trade-offs

The unified `/mcp` adds `preview_report`, `preview_echarts` and `commit_chart`. Pure request contracts
live in `quire-server`; runtime implements the existing external harness bridge through a shared
`PreviewToolService`. Native Rig, report rmcp and the administrator report-preview adapter share the
same execution result: JSON diagnostics plus an optional image. External calls use trusted session/user
metadata, transient chart drafts carry owner/session fields, and commits enforce document or chat ACLs.
Legacy ownerless drafts cannot be refined or committed and must be regenerated.

The accepted cost is a broader internal bridge and transport-neutral result type. In return, external
authors receive the same controlled preview workflow without adding runtime dependencies to the server.
