use super::*;

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
        if i64::try_from(size_bytes).is_err() {
            return Err(RuntimeError::InvalidRequest(
                "attachment size_bytes is out of range".into(),
            ));
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
        if let Some(object_key) = upload.object_key {
            let object_key = object_key.trim();
            self.validate_oss_object_key(upload.session_id, object_key)?;
            return self
                .store
                .insert_attachment(AttachmentInsert {
                    session_id: upload.session_id,
                    attachment_id,
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
}
