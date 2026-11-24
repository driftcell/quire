use super::*;

/// Execute Python, JavaScript, or Bash code inside this run's isolated remote sandbox.
///
/// The sandbox is provisioned on first use, persists files and installed dependencies across
/// calls within the same run, and is destroyed when the run ends. Returns `exit_code`, `stdout`,
/// and `stderr` (each stream is capped at 64 KiB with `stdout_truncated` / `stderr_truncated` flags).
#[rig_tool(name = "sandbox_execute")]
pub(crate) async fn sandbox_execute_tool(
    #[rig(context)] context: &mut ToolContext,
    /// Execution runtime: `"python"`, `"javascript"`, or `"bash"`.
    language: String,
    /// Source code or shell script to execute (up to 256 KiB).
    code: String,
    /// Optional working directory inside the sandbox container.
    cwd: Option<String>,
    /// Optional execution timeout in seconds.
    timeout_seconds: Option<u64>,
) -> Result<SandboxExecuteReceipt, ToolExecutionError> {
    if code.len() > MAX_TOOL_CODE_BYTES {
        return Err(ToolExecutionError::invalid_args(format!(
            "code exceeds the {MAX_TOOL_CODE_BYTES}-byte tool limit"
        )));
    }
    let runtime = runtime_tool_scope(context)?;
    let sandbox = runtime.sandbox.get_or_create().await.map_err(tool_other)?;
    let result = sandbox
        .exec_code(SandboxExecRequest {
            language,
            code,
            cwd,
            timeout_seconds,
        })
        .await
        .map_err(tool_other)?;
    let (stdout, stdout_bounded) = bounded_tool_text(result.stdout);
    let (stderr, stderr_bounded) = bounded_tool_text(result.stderr);
    Ok(SandboxExecuteReceipt {
        sandbox_id: sandbox.id().to_string(),
        exit_code: result.exit_code,
        stdout,
        stderr,
        stdout_truncated: result.stdout_truncated || stdout_bounded,
        stderr_truncated: result.stderr_truncated || stderr_bounded,
    })
}

/// Read a file (up to 256 KiB) from this run's sandbox filesystem.
///
/// UTF-8 text is returned directly (`encoding: "utf-8"`); binary files are returned as
/// `encoding: "base64"`. Reading a file larger than 256 KiB will fail—use `sandbox_execute`
/// with Python or Bash to inspect or chunk larger files.
#[rig_tool(name = "sandbox_read_file")]
pub(crate) async fn sandbox_read_file_tool(
    #[rig(context)] context: &mut ToolContext,
    /// Absolute or workspace-relative file path inside the sandbox (must be `<= 256` KiB).
    path: String,
) -> Result<SandboxReadFileReceipt, ToolExecutionError> {
    if path.trim().is_empty() {
        return Err(ToolExecutionError::invalid_args(
            "path cannot be empty".to_string(),
        ));
    }
    let runtime = runtime_tool_scope(context)?;
    let sandbox = runtime.sandbox.get_or_create().await.map_err(tool_other)?;
    let bytes = sandbox.download_file(&path).await.map_err(tool_other)?;
    if bytes.len() > MAX_TOOL_FILE_BYTES {
        return Err(ToolExecutionError::other(format!(
            "file exceeds the {MAX_TOOL_FILE_BYTES}-byte tool limit; read a chunk with sandbox_execute"
        )));
    }
    let (encoding, content) = match String::from_utf8(bytes) {
        Ok(text) => ("utf-8".to_string(), text),
        Err(error) => ("base64".to_string(), BASE64.encode(error.into_bytes())),
    };
    Ok(SandboxReadFileReceipt {
        sandbox_id: sandbox.id().to_string(),
        path,
        encoding,
        content,
    })
}

/// Write a UTF-8 text file (up to 256 KiB) in this run's sandbox filesystem, creating missing
/// parent directories when supported. For files exceeding 256 KiB or binary outputs, generate
/// them via `sandbox_execute`.
#[rig_tool(name = "sandbox_write_file")]
pub(crate) async fn sandbox_write_file_tool(
    #[rig(context)] context: &mut ToolContext,
    /// Destination file path inside the sandbox.
    path: String,
    /// UTF-8 text content to write (up to 256 KiB).
    content: String,
) -> Result<SandboxWriteFileReceipt, ToolExecutionError> {
    if path.trim().is_empty() {
        return Err(ToolExecutionError::invalid_args(
            "path cannot be empty".to_string(),
        ));
    }
    if content.len() > MAX_TOOL_FILE_BYTES {
        return Err(ToolExecutionError::invalid_args(format!(
            "content exceeds the {MAX_TOOL_FILE_BYTES}-byte tool limit"
        )));
    }
    let runtime = runtime_tool_scope(context)?;
    let sandbox = runtime.sandbox.get_or_create().await.map_err(tool_other)?;
    sandbox
        .upload_file(&path, content.as_bytes())
        .await
        .map_err(tool_other)?;
    Ok(SandboxWriteFileReceipt {
        sandbox_id: sandbox.id().to_string(),
        path,
        bytes_written: content.len(),
    })
}

/// List all attachments uploaded to the current session, returning each attachment's
/// `attachment_id`, `filename`, `mime_type`, and `size_bytes`. Pass `attachment_id`
/// to `sandbox_mount_attachment` when you need to inspect or process a file in the sandbox.
#[rig_tool(name = "list_attachments")]
pub(crate) async fn list_attachments_tool(
    #[rig(context)] context: &mut ToolContext,
) -> Result<ListAttachmentsReceipt, ToolExecutionError> {
    let runtime = runtime_tool_scope(context)?;
    let turn = quire_turn_meta(context)?;
    let attachments = runtime
        .engine
        .kb
        .store()
        .list_attachments(&turn.session_id)
        .await
        .map_err(tool_other)?
        .into_iter()
        .map(|att| AttachmentSummaryView {
            attachment_id: att.attachment_id,
            filename: att.filename,
            mime_type: att.mime_type,
            size_bytes: att.size_bytes,
        })
        .collect();
    Ok(ListAttachmentsReceipt { attachments })
}

/// Mount an attachment uploaded to the current session into the run's sandbox at
/// `/workspace/attachments/{attachment_id}/{filename}`.
///
/// Returns the mounted sandbox `path`, `filename`, `mime_type`, and `size_bytes` without
/// exposing storage credentials or signed URLs. After mounting, read small text files (`<= 256` KiB)
/// with `sandbox_read_file` or parse spreadsheets/PDFs/large datasets with `sandbox_execute`.
#[rig_tool(name = "sandbox_mount_attachment")]
pub(crate) async fn sandbox_mount_attachment_tool(
    #[rig(context)] context: &mut ToolContext,
    /// Session attachment ID to mount into `/workspace/attachments/{attachment_id}/{filename}`.
    attachment_id: String,
) -> Result<SandboxMountAttachmentReceipt, ToolExecutionError> {
    let runtime = runtime_tool_scope(context)?;
    let turn = quire_turn_meta(context)?;
    let attachment_id = safe_attachment_id(&attachment_id).map_err(tool_other)?;
    let stored = runtime
        .engine
        .kb
        .store()
        .get_session_attachment(&turn.session_id, attachment_id)
        .await
        .map_err(tool_other)?;
    let attachment_id = safe_attachment_id(&stored.attachment_id).map_err(tool_other)?;
    let filename = safe_attachment_filename(&stored.filename).map_err(tool_other)?;
    let directory = format!("/workspace/attachments/{attachment_id}");
    let target_path = format!("{directory}/{filename}");
    let sandbox = runtime.sandbox.get_or_create().await.map_err(tool_other)?;

    match stored.storage_provider.as_str() {
        "oss" => {
            let object_key = stored.storage_path.as_str();
            if !runtime
                .engine
                .config
                .oss
                .owns_object_key(&turn.session_id, object_key)
            {
                return Err(ToolExecutionError::other(
                    "attachment object key does not belong to this session",
                ));
            }
            let signed_url = runtime
                .engine
                .config
                .oss
                .presign_get_for_sandbox(object_key, SANDBOX_ATTACHMENT_GET_TTL_SECS)
                .map_err(tool_other)?;
            let command = format!(
                "set -euo pipefail\n\
                 if ! mkdir -p {directory} 2>/dev/null; then\n\
                   sudo -n mkdir -p {directory}\n\
                   sudo -n chown \"$(id -u):$(id -g)\" /workspace\n\
                   sudo -n chown -R \"$(id -u):$(id -g)\" {directory}\n\
                 fi\n\
                 curl -fsSL --retry 2 --connect-timeout 15 --max-time 120 -o {target} {url}",
                directory = shell_quote(&directory),
                target = shell_quote(&target_path),
                url = shell_quote(&signed_url),
            );
            let result = sandbox
                .exec_code(SandboxExecRequest {
                    language: "bash".into(),
                    code: command,
                    cwd: None,
                    timeout_seconds: Some(150),
                })
                .await
                .map_err(tool_other)?;
            if result.exit_code != 0 {
                return Err(ToolExecutionError::other(format!(
                    "failed to download attachment from OSS (exit code {}): {}",
                    result.exit_code,
                    sanitize_oss_download_error(&result.stderr, &signed_url)
                )));
            }
        }
        "local" => {
            let bytes = tokio::fs::read(&stored.storage_path)
                .await
                .map_err(|error| {
                    tool_other(format!("failed to read local attachment file: {error}"))
                })?;
            sandbox
                .upload_file(&target_path, &bytes)
                .await
                .map_err(tool_other)?;
        }
        provider => {
            return Err(ToolExecutionError::other(format!(
                "unsupported attachment storage provider `{provider}`"
            )));
        }
    }

    Ok(SandboxMountAttachmentReceipt {
        sandbox_id: sandbox.id().to_string(),
        attachment_id: attachment_id.to_string(),
        path: target_path,
        filename: filename.to_string(),
        mime_type: stored.mime_type,
        size_bytes: stored.size_bytes,
    })
}

pub(crate) fn shell_quote(value: &str) -> String {
    let mut quoted = String::with_capacity(value.len().saturating_add(2));
    quoted.push('\'');
    for character in value.chars() {
        if character == '\'' {
            quoted.push_str("'\\''");
        } else {
            quoted.push(character);
        }
    }
    quoted.push('\'');
    quoted
}

/// Remove any URL and signed query values before an OSS download error can
/// reach the model, the persisted tool call or a RunTrace preview.
pub(crate) fn sanitize_oss_download_error(stderr: &str, signed_url: &str) -> String {
    let mut sanitized = stderr.replace(signed_url, "[REDACTED_OSS_URL]");
    sanitized = redact_urls(&sanitized);
    for parameter in [
        "x-oss-signature=",
        "x-oss-credential=",
        "x-oss-security-token=",
    ] {
        sanitized = redact_query_parameter(&sanitized, parameter);
    }
    let bounded: String = sanitized.trim().chars().take(2_000).collect();
    if bounded.is_empty() {
        "OSS download command failed without diagnostic output".into()
    } else {
        bounded
    }
}

fn redact_urls(text: &str) -> String {
    let mut output = String::with_capacity(text.len());
    let mut rest = text;
    loop {
        let position = match (rest.find("https://"), rest.find("http://")) {
            (Some(left), Some(right)) => left.min(right),
            (Some(left), None) => left,
            (None, Some(right)) => right,
            (None, None) => break,
        };
        let (before, from_url) = rest.split_at(position);
        output.push_str(before);
        output.push_str("[REDACTED_OSS_URL]");
        let end = from_url
            .find(|character: char| {
                character.is_whitespace() || matches!(character, '\'' | '"' | ')' | ']' | '>' | '<')
            })
            .unwrap_or(from_url.len());
        if end == 0 {
            break;
        }
        rest = from_url.get(end..).unwrap_or_default();
    }
    output.push_str(rest);
    output
}

fn redact_query_parameter(text: &str, parameter: &str) -> String {
    let mut output = String::with_capacity(text.len());
    let mut rest = text;
    while let Some(position) = rest.find(parameter) {
        let (before, from_parameter) = rest.split_at(position);
        output.push_str(before);
        output.push_str("[REDACTED_OSS_QUERY]");
        let value_start = parameter.len();
        let end = from_parameter
            .get(value_start..)
            .and_then(|value| {
                value.find(|character: char| {
                    character.is_whitespace()
                        || matches!(character, '&' | '\'' | '"' | ')' | ']' | '>' | '<')
                })
            })
            .map(|offset| value_start.saturating_add(offset))
            .unwrap_or(from_parameter.len());
        if end <= value_start {
            rest = from_parameter.get(value_start..).unwrap_or_default();
            continue;
        }
        rest = from_parameter.get(end..).unwrap_or_default();
    }
    output.push_str(rest);
    output
}
