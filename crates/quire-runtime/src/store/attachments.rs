use super::*;

impl RuntimeStore {
    pub async fn insert_attachment(
        &self,
        attachment: AttachmentInsert<'_>,
    ) -> RuntimeResult<AttachmentMeta> {
        let attachment_id = attachment
            .attachment_id
            .map(str::to_string)
            .unwrap_or_else(|| short_id("att"));
        sqlx::query(
            r#"INSERT INTO agent_attachments
               (attachment_id, session_id, filename, mime_type, size_bytes, storage_path,
                storage_provider)
               VALUES ($1, $2, $3, $4, $5, $6, $7)"#,
        )
        .bind(&attachment_id)
        .bind(attachment.session_id)
        .bind(attachment.filename)
        .bind(attachment.mime_type)
        .bind(i64::try_from(attachment.size_bytes).unwrap_or(i64::MAX))
        .bind(attachment.storage_path)
        .bind(attachment.storage_provider)
        .execute(&self.pool)
        .await?;
        Ok(AttachmentMeta {
            attachment_id,
            filename: attachment.filename.to_string(),
            mime_type: attachment.mime_type.to_string(),
            size_bytes: attachment.size_bytes,
        })
    }

    /// Load an attachment only when it belongs to the requested session.
    pub async fn get_session_attachment(
        &self,
        session_id: &str,
        attachment_id: &str,
    ) -> RuntimeResult<StoredAttachment> {
        let row = sqlx::query(
            r#"SELECT attachment_id, session_id, filename, mime_type, size_bytes,
                      storage_path, storage_provider
               FROM agent_attachments
               WHERE attachment_id = $1 AND session_id = $2"#,
        )
        .bind(attachment_id)
        .bind(session_id)
        .fetch_optional(&self.pool)
        .await?
        .ok_or_else(|| RuntimeError::AttachmentNotFound(attachment_id.to_string()))?;
        let size_bytes: i64 = row.try_get("size_bytes")?;
        Ok(StoredAttachment {
            attachment_id: row.try_get("attachment_id")?,
            session_id: row.try_get("session_id")?,
            filename: row.try_get("filename")?,
            mime_type: row.try_get("mime_type")?,
            size_bytes: u64::try_from(size_bytes).unwrap_or(0),
            storage_path: row.try_get("storage_path")?,
            storage_provider: row.try_get("storage_provider")?,
        })
    }

    pub async fn list_attachments(&self, session_id: &str) -> RuntimeResult<Vec<AttachmentMeta>> {
        let rows = sqlx::query(
            r#"SELECT attachment_id, filename, mime_type, size_bytes
               FROM agent_attachments WHERE session_id = $1 ORDER BY created_at ASC"#,
        )
        .bind(session_id)
        .fetch_all(&self.pool)
        .await?;
        rows.iter()
            .map(|row| {
                let size_bytes: i64 = row.try_get("size_bytes")?;
                Ok(AttachmentMeta {
                    attachment_id: row.try_get("attachment_id")?,
                    filename: row.try_get("filename")?,
                    mime_type: row.try_get("mime_type")?,
                    size_bytes: u64::try_from(size_bytes).unwrap_or(0),
                })
            })
            .collect()
    }
}
