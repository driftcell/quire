# RUNTIME SANDBOX

## Metadata

- **Status:** Settled
- **Owner Crates:** `quire-runtime`
- **Related Notes:** `../notes/fc_sandbox_provider.md`

## 1. Scope & Non-Goals

This document governs the vendor-neutral sandbox port, environment-based provider selection,
run-scoped lifecycle, rig built-in tools and the Alibaba Cloud FC Agent Sandbox implementation in
`quire-runtime`. It does not cover Quire report MCP tools, remote MCP registry entries, durable
cross-turn artifact storage, browser automation or sandbox template construction.

## 2. Canonical Design & Contract

### Provider port and configuration

`crates/quire-runtime/src/sandbox.rs` defines `SandboxProvider`, which creates a `Sandbox` handle.
The handle owns `exec_code`, byte file upload/download and idempotent destruction. `SandboxLease`
lazily creates and memoizes one handle per agent run. `LocalSandboxProvider` is an explicit test
stub; `DisabledSandboxProvider` is the default and never falls back to host-shell execution.

`SandboxConfig::from_env` and `build_sandbox_provider` support:

| Variable | Default | Contract |
| --- | --- | --- |
| `SANDBOX_PROVIDER` | `disabled` | `disabled`/`none`, `local` or `fc` (`aliyun-fc` is an alias) |
| `SANDBOX_TIMEOUT_SECONDS` | `300` | Remote sandbox TTL, clamped to 60–86400 seconds |
| `SANDBOX_FC_API_KEY` | — | Required FC data-plane E2B API key; stored as `SecretString` |
| `SANDBOX_FC_REGION` | `cn-hangzhou` | Region used to derive the default FC endpoints |
| `SANDBOX_FC_API_URL` | derived from region | Optional platform API override |
| `SANDBOX_FC_DOMAIN` | derived from region | Optional sandbox routing domain override |
| `SANDBOX_FC_SANDBOX_URL` | derived from sandbox ID/domain | Optional envd base URL override, primarily for compatible endpoints/tests |
| `SANDBOX_FC_TEMPLATE` | `code-interpreter-v1` | Template used when the run specification does not override it |

Unknown providers and FC selected without an API key fail runtime startup. Configuration `Debug`
output redacts the API key. The Alibaba Cloud AccessKey pair used for control-plane administration
is not a runtime setting and is not interchangeable with `SANDBOX_FC_API_KEY`.

### rig tools and lifecycle

`RigModelStreamProvider::new` receives an `Arc<dyn SandboxProvider>`. `rig_agent.rs` adds three
allow-listed built-ins, with the same wildcard, event persistence and cancellation behavior as
other runtime tools:

- `sandbox_execute`: Python, JavaScript or Bash source, optional working directory and timeout.
- `sandbox_read_file`: UTF-8 text or base64 binary output, limited to 256 KiB per tool call.
- `sandbox_write_file`: UTF-8 content, limited to 256 KiB per tool call.

The first sandbox call in a run creates the instance; subsequent calls reuse its filesystem and
installed dependencies. Source execution is process-based, not a Jupyter state contract. Tool text
output is capped at 64 KiB per stream, with explicit truncation flags. Every run and subagent run
gets an independent lease. Normal run completion awaits destruction; early return/cancellation uses
best-effort scheduled cleanup, and the remote TTL is the final backstop. Sandboxes do not persist
implicitly across user turns.

### Alibaba Cloud FC adapter

`crates/quire-runtime/src/sandbox/fc.rs` keeps FC/E2B protocol details behind the port:

- `POST /sandboxes` with `X-API-Key`, `secure: true`, `autoPause: false`, template, TTL and metadata.
- `DELETE /sandboxes/{id}` for idempotent cleanup.
- Envd routing through `E2b-Sandbox-Id`, `E2b-Sandbox-Port` and optional `X-Access-Token`.
- Multipart `POST /files` and bounded `GET /files` for byte file I/O.
- `process.Process/Start` using Connect JSON envelopes for process execution, including fragmented
  frame decoding, base64 stdout/stderr, exit status and output bounds.

Platform, envd and cleanup errors report the operation and HTTP status without echoing response
bodies or credentials. The adapter supports Python (`python3 -c`), JavaScript (`node -e`) and Bash
(`/bin/bash -l -c`) without concatenating source into a shell command for Python or JavaScript.
Create, file, process and destroy requests have explicit timeouts. `quire-server` and protocol
crates have no dependency on this implementation.

## 3. Invariants & Verification

- No sandbox environment configuration means disabled behavior, never host execution.
- rig depends only on the provider port and never names FC/E2B types.
- One run reuses one sandbox; separate runs do not share mutable sandbox state.
- Destruction is idempotent and every created remote sandbox has a TTL.
- API keys and envd access tokens remain secret and are absent from debug/error output.
- Connect decoding handles arbitrary network chunk boundaries and rejects malformed/oversized data.
- Mock HTTP coverage proves platform creation, auth headers, process streaming, file I/O and cleanup.
- Unit coverage also proves lazy lease reuse, local file round trips, fail-closed provider selection,
  rig schemas, output limits and configuration redaction.

Full gate:

```bash
cargo fmt --all -- --check
cargo clippy --workspace --all-targets -- -D warnings
cargo test --workspace
```

A live FC check is a separate operational verification: create a short-lived sandbox in the selected
region, execute Python and Bash, round-trip a file and destroy both the sandbox and temporary API
key. Live credentials and smoke-test scripts stay outside the repository and never replace the
hermetic protocol tests.
