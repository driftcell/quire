# FC SANDBOX PROVIDER

## Metadata

- **Status:** Accepted
- **Date:** 2026-11-19
- **Promoted To:** `../docs/RUNTIME_SANDBOX.md`

## 1. Context & Problem

`quire-runtime` already contained a `SandboxProvider` trait and a local stub, but the trait was not
connected to rig tools and had no production provider. The runtime needs isolated command/code
execution and file access for agents while keeping rig, session orchestration and tool persistence
independent of any sandbox vendor. Alibaba Cloud FC Agent Sandbox is the first production provider;
its E2B-compatible API uses a platform REST API plus envd file and Connect streaming process APIs.
Configuration must be opt-in through `SANDBOX_*` environment variables, and missing configuration
must not silently execute commands on the host.

## 2. Rejected Options & Why

- **Expose FC/E2B directly as rig tools.** Rejected because it would leak vendor template, endpoint,
  token and envd details into the agent loop and make a second provider a rig-level integration.
- **Run a Python or TypeScript SDK sidecar and register it over MCP.** Rejected for the native path
  because it adds another deployed service and process lifecycle without improving the existing
  built-in tool/event/cancellation path. MCP remains a valid external integration option, not the
  default FC implementation.
- **Spawn an SDK process for every operation.** Rejected because repeated interpreter startup,
  dependency management and credential forwarding are unreliable, and per-call sandbox creation would
  lose files and process state between model tool calls.
- **Depend on an unrelated agent runtime or an old Rust E2B crate.** Rejected because those packages
  either duplicate rig, expose incompatible lifecycle defaults, or make credential/lifecycle types
  part of another agent framework. The small platform/envd surface is kept inside our adapter.
- **Default to the local stub or host shell.** Rejected because a stub success result is not code
  execution, and an unset remote provider must never turn into an unintended host execution path.
- **Create one sandbox per tool call or keep one global sandbox.** Rejected because per-call creation
  loses state and adds latency, while a global instance mixes tenants, sessions and concurrent runs.

## 3. Chosen Approach & Trade-offs

Keep a vendor-neutral `SandboxProvider` factory that returns a `Sandbox` handle. Handles own command
execution, byte file I/O and destruction. `LocalSandboxProvider` remains an explicit test provider;
`FcSandboxProvider` implements Alibaba Cloud FC through the E2B-compatible platform and envd APIs.
A `SANDBOX_*` configuration factory selects `disabled` (default), `local` or `fc`; secrets are held in
`SecretString` and redacted from configuration debug output.

rig exposes allow-listed built-in sandbox tools through a run-scoped lazy lease. The first sandbox
tool call creates the remote instance, later calls in the same run reuse it, and run completion or
cancellation destroys it best-effort. The remote TTL is the final cleanup backstop. Subagent runs get
independent leases rather than sharing mutable files with their parent.

The accepted costs are maintaining a small Connect JSON framing decoder in the FC adapter, buffering
command output for the current synchronous tool contract, and run-scoped rather than cross-turn
session persistence. Cross-turn persistence, durable artifact storage, streaming process output and
rich Jupyter results are separate follow-up designs, not implicit behavior of this step.
