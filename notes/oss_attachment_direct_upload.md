# OSS ATTACHMENT DIRECT UPLOAD

## Metadata

- **Status:** Accepted
- **Date:** 2026-11-20
- **Promoted To:** `../docs/RUNTIME_OSS_ATTACHMENTS.md`

## 1. Context & Problem

Attachments previously travelled through the runtime as base64 JSON and were written to the local
filesystem. Large files inflate frontend memory and runtime request bodies, and a remote FC sandbox
cannot read the runtime's local disk. The runtime needs a browser-direct OSS upload path plus a
server-side, short-lived download URL for sandbox mounting, while keeping credentials and signed
URLs out of model context and RunTrace.

## 2. Rejected Options & Why

- **Keep whole-file base64 uploads for every attachment.** Rejected for OSS mode because it doubles
  memory use in the browser and runtime and makes large uploads fragile.
- **Add the Alibaba Cloud OSS SDK or another cryptography crate.** Rejected because the workspace
  already ships `ring`; an SDK would add a large dependency surface for one signature format.
- **Let the browser sign URLs with the AccessKey secret.** Rejected because the secret would be
  exposed to every client.
- **Sign with OSS V1 signatures.** Rejected because V4 is the current recommended mechanism and V1
  is being retired.
- **Return `storage_provider` or `storage_path` in `AttachmentMeta`.** Rejected because storage
  provenance is not part of the browser or model contract and would leak bucket/object layout.
- **Mount OSS objects by exposing the bucket or credentials to the sandbox.** Rejected because the
  sandbox should receive one short-lived URL, not durable storage credentials.
- **Put the signed URL in tool arguments, stdout or trace metadata.** Rejected because tool inputs
  and outputs are persisted and shown in the UI; a fresh URL is signed only inside the tool call.
- **Trust a client-supplied object key directly.** Rejected because it would permit cross-session
  confirmation; the key must match the configured prefix and the current session.

## 3. Chosen Approach & Trade-offs

Presign with a small pure-Rust V4 implementation built on `ring`. The API returns the URL only to
the uploading browser and stores just the object key plus the `oss` provider marker. The
`sandbox_mount_attachment` tool re-checks session ownership, signs a 600-second GET URL on demand,
and passes it to `curl` inside the sandbox. Success payloads and sanitized failures never contain
the URL or query signature values.

The accepted trade-off is that OSS mode depends on bucket CORS configuration for browser `PUT`, and
the runtime does not yet clean up orphaned objects after aborted uploads. Those are operational
concerns outside the attachment contract; local mode remains the default whenever OSS is
incomplete.
