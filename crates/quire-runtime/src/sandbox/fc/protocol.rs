use super::*;

pub(crate) fn encode_connect_frame<T: Serialize>(message: &T) -> Result<Vec<u8>, SandboxError> {
    let payload = serde_json::to_vec(message)
        .map_err(|_| SandboxError::Failed("failed to serialize Connect request".into()))?;
    let length = u32::try_from(payload.len())
        .map_err(|_| SandboxError::InvalidInput("Connect request is too large".into()))?;
    let mut frame = Vec::with_capacity(payload.len().saturating_add(5));
    frame.push(0);
    frame.extend_from_slice(&length.to_be_bytes());
    frame.extend_from_slice(&payload);
    Ok(frame)
}

#[derive(Default)]
pub(crate) struct ConnectFrameDecoder {
    buffer: Vec<u8>,
}

impl ConnectFrameDecoder {
    pub(crate) fn push(&mut self, chunk: &[u8]) {
        self.buffer.extend_from_slice(chunk);
    }

    pub(crate) fn drain(&mut self) -> Result<Vec<ConnectFrame>, SandboxError> {
        let mut frames = Vec::new();
        let mut offset = 0usize;
        while self.buffer.len().saturating_sub(offset) >= 5 {
            let header = self
                .buffer
                .get(offset..offset.saturating_add(5))
                .ok_or_else(|| SandboxError::Failed("truncated Connect header".into()))?;
            let flags = header
                .first()
                .copied()
                .ok_or_else(|| SandboxError::Failed("missing Connect flags".into()))?;
            let length_bytes: [u8; 4] = header
                .get(1..5)
                .ok_or_else(|| SandboxError::Failed("truncated Connect length".into()))?
                .try_into()
                .map_err(|_| SandboxError::Failed("invalid Connect length".into()))?;
            let length = u32::from_be_bytes(length_bytes) as usize;
            if length > MAX_CONNECT_FRAME_BYTES {
                return Err(SandboxError::Failed(
                    "Connect frame exceeded the size limit".into(),
                ));
            }
            let start = offset.saturating_add(5);
            let end = start.saturating_add(length);
            if self.buffer.len() < end {
                break;
            }
            let payload = self
                .buffer
                .get(start..end)
                .ok_or_else(|| SandboxError::Failed("truncated Connect payload".into()))?;
            let value = serde_json::from_slice::<Value>(payload)
                .map_err(|_| SandboxError::Failed("invalid Connect JSON payload".into()))?;
            frames.push(if flags & 0x02 != 0 {
                ConnectFrame::EndStream(value)
            } else {
                ConnectFrame::Message(value)
            });
            offset = end;
        }
        self.buffer.drain(..offset);
        Ok(frames)
    }
}

pub(crate) async fn collect_process_output(
    response: Response,
) -> Result<SandboxExecResult, SandboxError> {
    let mut stdout = Vec::new();
    let mut stderr = Vec::new();
    let mut stdout_truncated = false;
    let mut stderr_truncated = false;
    let mut exit_code = None;
    let mut decoder = ConnectFrameDecoder::default();
    let mut stream = response.bytes_stream();
    while let Some(chunk) = stream.next().await {
        let chunk =
            chunk.map_err(|_| SandboxError::Failed("failed to read FC process stream".into()))?;
        decoder.push(&chunk);
        for frame in decoder.drain()? {
            match frame {
                ConnectFrame::Message(value) => apply_process_event(
                    &value,
                    &mut stdout,
                    &mut stderr,
                    &mut stdout_truncated,
                    &mut stderr_truncated,
                    &mut exit_code,
                )?,
                ConnectFrame::EndStream(value) => {
                    if let Some(error) = connect_stream_error(&value) {
                        return Err(SandboxError::Failed(format!(
                            "FC envd rejected the command: {error}"
                        )));
                    }
                }
            }
        }
    }
    let exit_code = exit_code.ok_or_else(|| {
        SandboxError::Failed("FC process stream ended before the process finished".into())
    })?;
    Ok(SandboxExecResult {
        exit_code,
        stdout: String::from_utf8_lossy(&stdout).into_owned(),
        stderr: String::from_utf8_lossy(&stderr).into_owned(),
        stdout_truncated,
        stderr_truncated,
    })
}

fn append_output(
    sink: &mut Vec<u8>,
    encoded: &str,
    truncated: &mut bool,
) -> Result<(), SandboxError> {
    let bytes = BASE64
        .decode(encoded)
        .map_err(|_| SandboxError::Failed("invalid base64 in FC process output".into()))?;
    let remaining = MAX_OUTPUT_BYTES.saturating_sub(sink.len());
    if bytes.len() > remaining {
        *truncated = true;
    }
    sink.extend(bytes.iter().take(remaining));
    Ok(())
}

fn apply_process_event(
    value: &Value,
    stdout: &mut Vec<u8>,
    stderr: &mut Vec<u8>,
    stdout_truncated: &mut bool,
    stderr_truncated: &mut bool,
    exit_code: &mut Option<i32>,
) -> Result<(), SandboxError> {
    let Some(event) = value.get("event") else {
        return Ok(());
    };
    if let Some(data) = event.get("data") {
        if let Some(encoded) = data.get("stdout").and_then(Value::as_str) {
            append_output(stdout, encoded, stdout_truncated)?;
        }
        if let Some(encoded) = data.get("stderr").and_then(Value::as_str) {
            append_output(stderr, encoded, stderr_truncated)?;
        }
        if let Some(encoded) = data.get("pty").and_then(Value::as_str) {
            append_output(stdout, encoded, stdout_truncated)?;
        }
    }
    if let Some(end) = event.get("end") {
        let code = end
            .get("exitCode")
            .and_then(|value| {
                value
                    .as_i64()
                    .or_else(|| value.as_str().and_then(|raw| raw.parse::<i64>().ok()))
            })
            .unwrap_or(0);
        *exit_code = Some(i32::try_from(code).unwrap_or(-1));
    }
    Ok(())
}

fn connect_stream_error(value: &Value) -> Option<String> {
    let error = value.get("error")?;
    let code = error
        .get("code")
        .and_then(Value::as_str)
        .unwrap_or("unknown");
    let message = error
        .get("message")
        .and_then(Value::as_str)
        .unwrap_or("unknown error")
        .chars()
        .take(500)
        .collect::<String>();
    Some(format!("{code}: {message}"))
}
