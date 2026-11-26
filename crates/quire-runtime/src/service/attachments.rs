use crate::bus::OssPresignTicket;

use super::*;

/// Maximum single attachment upload size (50 MiB).
pub const MAX_ATTACHMENT_SIZE_BYTES: u64 = 50 * 1024 * 1024;

impl RuntimeService {
    pub async fn presign_attachment(
        &self,
        session_id: &str,
        filename: &str,
        mime_type: &str,
        size_bytes: u64,
    ) -> RuntimeResult<AttachmentPresign> {
        self.store.get_session_summary(session_id).await?;
        let filename = safe_attachment_filename(filename)?;
        let mime_type = mime_type.trim();
        if mime_type.is_empty() {
            return Err(RuntimeError::InvalidRequest(
                "attachment mime_type must not be empty".into(),
            ));
        }
        if size_bytes == 0
            || size_bytes > MAX_ATTACHMENT_SIZE_BYTES
            || i64::try_from(size_bytes).is_err()
        {
            return Err(RuntimeError::InvalidRequest(format!(
                "attachment size_bytes must be between 1 and {MAX_ATTACHMENT_SIZE_BYTES} bytes"
            )));
        }
        if !self.config.oss.is_configured() {
            return Ok(AttachmentPresign::Inline);
        }
        let attachment_id = short_id("att");
        let object_key = self
            .config
            .oss
            .object_key(session_id, &attachment_id, filename);
        let upload_url =
            self.config
                .oss
                .presign_put(&object_key, mime_type, ATTACHMENT_PRESIGN_TTL_SECS)?;
        self.store
            .bus()
            .store_oss_presign_ticket(
                &OssPresignTicket {
                    session_id: session_id.to_string(),
                    attachment_id: attachment_id.clone(),
                    object_key: object_key.clone(),
                    filename: filename.to_string(),
                    mime_type: mime_type.to_string(),
                    size_bytes,
                },
                ATTACHMENT_PRESIGN_TTL_SECS,
            )
            .await?;
        let headers = BTreeMap::from([("Content-Type".to_string(), mime_type.to_string())]);
        Ok(AttachmentPresign::Oss {
            attachment_id,
            object_key,
            upload_url,
            headers,
        })
    }

    pub async fn create_attachment(
        &self,
        upload: AttachmentWrite<'_>,
    ) -> RuntimeResult<AttachmentMeta> {
        self.store.get_session_summary(upload.session_id).await?;
        let filename = safe_attachment_filename(upload.filename)?;
        let attachment_id = upload.attachment_id.map(safe_attachment_id).transpose()?;
        if upload.size_bytes > MAX_ATTACHMENT_SIZE_BYTES
            || i64::try_from(upload.size_bytes).is_err()
        {
            return Err(RuntimeError::InvalidRequest(format!(
                "attachment size_bytes exceeds maximum limit of {MAX_ATTACHMENT_SIZE_BYTES} bytes"
            )));
        }
        if let Some(object_key) = upload.object_key {
            let object_key = object_key.trim();
            if upload.size_bytes == 0 {
                return Err(RuntimeError::InvalidRequest(
                    "OSS attachment size_bytes must be greater than 0".into(),
                ));
            }
            self.validate_oss_object_key(upload.session_id, object_key)?;
            let ticket = self
                .store
                .bus()
                .take_oss_presign_ticket(upload.session_id, object_key)
                .await?
                .ok_or_else(|| {
                    RuntimeError::InvalidRequest(
                        "OSS attachment upload was not presigned or the presign ticket expired"
                            .into(),
                    )
                })?;
            if ticket.size_bytes != upload.size_bytes {
                return Err(RuntimeError::InvalidRequest(format!(
                    "confirmed size_bytes ({}) does not match presigned size_bytes ({})",
                    upload.size_bytes, ticket.size_bytes
                )));
            }
            if let Some(requested_id) = attachment_id
                && requested_id != ticket.attachment_id
            {
                return Err(RuntimeError::InvalidRequest(
                    "confirmed attachment_id does not match presigned attachment_id".into(),
                ));
            }
            if self.config.oss.is_live_endpoint() {
                self.verify_oss_uploaded_object(object_key, upload.size_bytes)
                    .await?;
            }
            return self
                .store
                .insert_attachment(AttachmentInsert {
                    session_id: upload.session_id,
                    attachment_id: Some(ticket.attachment_id.as_str()),
                    filename,
                    mime_type: upload.mime_type,
                    size_bytes: upload.size_bytes,
                    storage_path: object_key,
                    storage_provider: "oss",
                })
                .await;
        }

        let bytes = match upload.content_base64 {
            Some(encoded) => base64::engine::general_purpose::STANDARD
                .decode(encoded.trim())
                .map_err(|error| {
                    RuntimeError::InvalidRequest(format!("invalid base64 attachment: {error}"))
                })?,
            None => Vec::new(),
        };
        if u64::try_from(bytes.len()).unwrap_or(u64::MAX) > MAX_ATTACHMENT_SIZE_BYTES {
            return Err(RuntimeError::InvalidRequest(format!(
                "decoded attachment exceeds maximum size of {MAX_ATTACHMENT_SIZE_BYTES} bytes"
            )));
        }
        let storage_dir = self
            .config
            .workspace_dir
            .join(".quire-runtime")
            .join("attachments");
        tokio::fs::create_dir_all(&storage_dir).await?;
        let path = storage_dir.join(format!("{}-{filename}", short_token(12)));
        tokio::fs::write(&path, &bytes).await?;
        self.store
            .insert_attachment(AttachmentInsert {
                session_id: upload.session_id,
                attachment_id,
                filename,
                mime_type: upload.mime_type,
                size_bytes: upload.size_bytes,
                storage_path: &path.to_string_lossy(),
                storage_provider: "local",
            })
            .await
    }

    fn validate_oss_object_key(&self, session_id: &str, object_key: &str) -> RuntimeResult<()> {
        if !self.config.oss.is_configured() {
            return Err(RuntimeError::InvalidRequest(
                "OSS attachments are not configured".into(),
            ));
        }
        if !self.config.oss.owns_object_key(session_id, object_key) {
            return Err(RuntimeError::InvalidRequest(
                "attachment object_key does not belong to this session".into(),
            ));
        }
        Ok(())
    }

    async fn verify_oss_uploaded_object(
        &self,
        object_key: &str,
        expected_size: u64,
    ) -> RuntimeResult<()> {
        let head_url = self.config.oss.presign_head(object_key, 60)?;
        let client = reqwest::Client::builder()
            .timeout(Duration::from_secs(10))
            .build()
            .map_err(|error| RuntimeError::Internal(format!("build OSS HTTP client: {error}")))?;
        let response = client.head(&head_url).send().await.map_err(|error| {
            RuntimeError::InvalidRequest(format!("failed to verify OSS object via HEAD: {error}"))
        })?;
        if !response.status().is_success() {
            return Err(RuntimeError::InvalidRequest(format!(
                "OSS object `{object_key}` was not uploaded (HEAD status {})",
                response.status()
            )));
        }
        if let Some(actual_len) = response
            .headers()
            .get(reqwest::header::CONTENT_LENGTH)
            .and_then(|v| v.to_str().ok())
            .and_then(|s| s.parse::<u64>().ok())
            && actual_len != expected_size
        {
            return Err(RuntimeError::InvalidRequest(format!(
                "OSS object size ({actual_len} bytes) does not match declared size_bytes ({expected_size} bytes)"
            )));
        }
        Ok(())
    }

    pub(crate) async fn delete_attachment_blob(
        &self,
        attachment: &StoredAttachment,
    ) -> RuntimeResult<()> {
        match attachment.storage_provider.as_str() {
            "local" => {
                match tokio::fs::remove_file(&attachment.storage_path).await {
                    Ok(()) => {}
                    Err(error) if error.kind() == std::io::ErrorKind::NotFound => {}
                    Err(error) => return Err(error.into()),
                }
                Ok(())
            }
            "oss" => {
                if !self.config.oss.is_configured() || !self.config.oss.is_live_endpoint() {
                    return Ok(());
                }
                let delete_url = self
                    .config
                    .oss
                    .presign_delete(&attachment.storage_path, 60)?;
                let client = reqwest::Client::builder()
                    .timeout(Duration::from_secs(10))
                    .build()
                    .map_err(|error| {
                        RuntimeError::Internal(format!("build OSS HTTP client: {error}"))
                    })?;
                let response = client.delete(&delete_url).send().await.map_err(|error| {
                    RuntimeError::Internal(format!("failed to delete OSS attachment: {error}"))
                })?;
                if !response.status().is_success()
                    && response.status() != reqwest::StatusCode::NOT_FOUND
                {
                    return Err(RuntimeError::Internal(format!(
                        "OSS DELETE `{}` failed with status {}",
                        attachment.storage_path,
                        response.status()
                    )));
                }
                Ok(())
            }
            _ => Ok(()),
        }
    }
}
