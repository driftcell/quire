# RUNTIME OSS ATTACHMENTS

## Metadata

- **Status:** Settled
- **Owner Crates:** `quire-runtime`, `web`
- **Related Notes:** `../notes/oss_attachment_direct_upload.md`

## 1. Scope & Non-Goals

This document governs browser-direct attachment uploads to Alibaba Cloud OSS, V4 URL presigning,
attachment storage provenance, and the `sandbox_mount_attachment` built-in tool. It does not cover
OSS lifecycle rules, CDN configuration, multipart uploads, STS temporary credentials, or deletion of
orphaned objects.

## 2. Canonical Design & Contract

### Configuration

`Config.oss` is an `OssConfig` populated by `OssConfig::from_env`:

| Variable | Contract |
| --- | --- |
| `QUIRE_OSS_BUCKET` | Bucket used for every attachment object |
| `QUIRE_OSS_REGION` | Region ID used in the V4 credential scope |
| `QUIRE_OSS_PUBLIC_ENDPOINT` | Browser-facing endpoint for presigned `PUT` URLs |
| `QUIRE_OSS_INTERNAL_ENDPOINT` | Optional VPC endpoint for sandbox `GET` URLs; public endpoint is the fallback |
| `OSS_ACCESS_KEY_ID` (or `QUIRE_OSS_ACCESS_KEY_ID`) | AccessKey ID |
| `OSS_ACCESS_KEY_SECRET` (or `QUIRE_OSS_ACCESS_KEY_SECRET`) | AccessKey secret, stored in `SecretString` |
| `QUIRE_OSS_KEY_PREFIX` | Object prefix, default `quire/attachments` |

`OssConfig::is_configured` requires bucket, region, public endpoint, AccessKey ID and secret. Any
missing value keeps the runtime in local attachment mode. `Debug` output redacts both credential
fields.

### V4 presigning

`crates/quire-runtime/src/oss.rs` implements Alibaba Cloud OSS V4 URL signatures with `ring::hmac`
and `ring::digest`; no SDK or additional cryptography crate is used.

- Browser uploads use `presign_put(object_key, content_type, 900)` against the public endpoint.
- Sandbox downloads use `presign_get_for_sandbox(object_key, 600)`, preferring the internal
  endpoint.
- Canonical URI is `/{bucket}/{uri-encoded-object-key}`; canonical headers include the exact
  `content-type` for uploads, while `host` stays unsigned (no `x-oss-additional-headers`).
- Query parameters are `x-oss-credential`, `x-oss-date`, `x-oss-expires` and
  `x-oss-signature-version`; `x-oss-signature` is appended after signing.
- The payload hash is `UNSIGNED-PAYLOAD`; the signing key is derived as
  `HMAC("aliyun_v4" + secret, date) -> region -> "oss" -> "aliyun_v4_request"`.
- Endpoints are parsed and validated; a bucket is prepended unless the endpoint host already starts
  with `{bucket}.`.

### Storage provenance

Migration `0007_attachment_storage_provider.sql` adds
`agent_attachments.storage_provider TEXT NOT NULL DEFAULT 'local'` with a `local`/`oss` check
constraint. `storage_path` stores a local filesystem path for `local` rows and the OSS object key for
`oss` rows. Neither column is part of `AttachmentMeta` or any browser/LLM trace contract.

`RuntimeStore::insert_attachment` accepts an optional pre-generated `attachment_id` and the provider.
`get_session_attachment(session_id, attachment_id)` uses exactly
`WHERE attachment_id = $1 AND session_id = $2`, so a valid ID from another session is indistinguishable
from a missing attachment.

### Upload endpoints

`POST /api/sessions/{session_id}/attachments/presign` accepts
`{ filename, mime_type, size_bytes }`. It verifies the session, sanitizes the filename, and returns:

```json
{
  "mode": "oss",
  "attachment_id": "att_...",
  "object_key": "quire/attachments/{session_id}/{attachment_id}/{filename}",
  "upload_url": "https://...",
  "headers": { "Content-Type": "text/csv" }
}
```

When OSS is not configured it returns `{ "mode": "inline" }`. The object key is generated as
`{prefix}/{session_id}/{attachment_id}/{safe_filename}`.

Browser direct upload requires a bucket CORS rule for the deployed web origin that allows
`PUT`, `GET` and `HEAD`, plus the `Content-Type` request header. Local verification used
`http://localhost:5173` and `http://127.0.0.1:5173`; add the production origin before deploying.

`POST /api/sessions/{session_id}/attachments` accepts optional `attachment_id` and `object_key`.
When `object_key` is present, OSS must be configured and the key must satisfy
`OssConfig::owns_object_key` (prefix `{prefix}/{session_id}/` plus a non-empty remainder). The row is
inserted with `storage_provider = "oss"` and `content_base64` is ignored. Without `object_key`, the
existing base64/local-file path is used and the row is stored as `local`.

### Browser upload

`web/src/api/runtime.ts` calls the presign endpoint first. For `mode === "oss"` it sends the `File`
directly with `PUT upload_url` and the returned headers, then confirms metadata through the normal
attachment endpoint. No whole-file base64 encoding occurs on this path. `mode === "inline"` keeps the
existing base64 fallback.

### Sandbox mount tool

`sandbox_mount_attachment` is allow-list controlled like other built-ins and accepts only
`{ attachment_id }`. It loads the attachment through the session-scoped store method, sanitizes the
stored filename, and targets
`/workspace/attachments/{attachment_id}/{filename}`.

- `local`: reads the stored file and calls `Sandbox::upload_file`.
- `oss`: validates the stored key against the session prefix, signs a 600-second GET URL on the
  server, then runs a bash script that creates the target directory and downloads with
  `curl -fsSL --retry 2`. FC sandboxes start without a writable `/workspace`; if the normal
  `mkdir -p` is denied, the script falls back to `sudo -n mkdir -p` and grants the sandbox user
  ownership of `/workspace` and the new attachment directory before retrying the download.

The success payload contains only `sandbox_id`, `attachment_id`, `path`, `filename`, `mime_type` and
`size_bytes`. The signed URL is not persisted, placed in tool arguments, returned to the model, or
included in a RunTrace. On failure, stderr is sanitized by removing full URLs and `x-oss-signature`,
`x-oss-credential` and `x-oss-security-token` values before the error reaches the tool result.

## 3. Invariants & Verification

- OSS is opt-in; missing configuration keeps local uploads working and never fails startup.
- AccessKey secrets are `SecretString` values and never appear in `Debug`, API records or traces.
- Signed URLs stay in the presign response or the server-to-sandbox process invocation only.
- Cross-session attachment reads fail through the combined `attachment_id`/`session_id` predicate.
- `sandbox_mount_attachment` returns a clean metadata object and a sanitized error on any failure.
- `storage_provider` and `storage_path` never enter `AttachmentMeta`, RunTrace or model context.

Verification:

```bash
cargo fmt --all -- --check
cargo clippy --workspace --all-targets -- -D warnings
cargo test --workspace
cd web && bun test && bun run typecheck && bun run lint && bun run build
```

Relevant tests: `oss.rs` V4 vectors, config Debug redaction, `attachments_presign_inline_and_local_mount`,
`attachments_oss_presign_and_object_key_scope`,
`sandbox_mount_attachment_scopes_sessions_and_redacts_traces`, and
`oss_download_errors_never_expose_signed_query_values`.
