use super::*;

impl PgReportStore {
    pub async fn document_at_revision(
        &self,
        doc_id: &DocId,
        revision: Revision,
    ) -> Result<ReportDocument, StoreError> {
        let current = self.get_document(doc_id).await?;
        if revision > current.revision {
            return Err(StoreError::InvalidRequest(format!(
                "revision {} is ahead of current revision {} for `{doc_id}`",
                revision.get(),
                current.revision.get()
            )));
        }
        if revision.is_zero() {
            let mut empty = ReportDocument::new(doc_id.clone());
            empty.revision = Revision::ZERO;
            return Ok(empty);
        }
        if revision == current.revision {
            return Ok(current);
        }
        let snapshot = sqlx::query!(
            r#"SELECT rev_num, snapshot_zstd FROM report_snapshots
               WHERE doc_id = $1 AND rev_num <= $2
               ORDER BY rev_num DESC LIMIT 1"#,
            doc_id.as_str(),
            revision.get()
        )
        .fetch_optional(&self.pool)
        .await?;
        if let Some(snapshot) = snapshot {
            let decoded = zstd::decode_all(snapshot.snapshot_zstd.as_slice())
                .map_err(|error| StoreError::Snapshot(error.to_string()))?;
            let mut document: ReportDocument = serde_json::from_slice(&decoded)?;
            document.revision = Revision::new(snapshot.rev_num)?;
            let rows = sqlx::query!(
                r#"SELECT rev_num, forward_ops FROM report_revisions
                   WHERE doc_id = $1 AND rev_num > $2 AND rev_num <= $3
                   ORDER BY rev_num ASC"#,
                doc_id.as_str(),
                snapshot.rev_num,
                revision.get()
            )
            .fetch_all(&self.pool)
            .await?;
            if i64::try_from(rows.len()).ok() != Some(revision.get() - snapshot.rev_num) {
                return Err(StoreError::CorruptRevision {
                    doc_id: doc_id.clone(),
                    revision,
                    message: "incomplete forward history for the requested snapshot".into(),
                });
            }
            for row in rows {
                let ops: Vec<PatchOp> = serde_json::from_value(row.forward_ops)?;
                apply_operations_in_place(&mut document, &ops)?;
                document.revision = Revision::new(row.rev_num)?;
            }
            return Ok(document);
        }

        let rows = sqlx::query!(
            r#"SELECT rev_num, inverse_ops FROM report_revisions
               WHERE doc_id = $1 AND rev_num > $2 AND rev_num <= $3
               ORDER BY rev_num DESC"#,
            doc_id.as_str(),
            revision.get(),
            current.revision.get()
        )
        .fetch_all(&self.pool)
        .await?;
        if i64::try_from(rows.len()).ok() != Some(current.revision.get() - revision.get()) {
            return Err(StoreError::CorruptRevision {
                doc_id: doc_id.clone(),
                revision,
                message: "incomplete inverse history for the requested snapshot".into(),
            });
        }
        let mut document = current;
        for row in rows {
            let ops: Vec<PatchOp> = serde_json::from_value(row.inverse_ops)?;
            apply_operations_in_place(&mut document, &ops)?;
            document.revision = Revision::new(row.rev_num.saturating_sub(1))?;
        }
        Ok(document)
    }

    pub(crate) fn maybe_snapshot(
        &self,
        doc_id: &str,
        revision: Revision,
        document: ReportDocument,
    ) {
        if revision.get() % 50 != 0 {
            return;
        }
        let pool = self.pool.clone();
        let doc_id = doc_id.to_string();
        tokio::spawn(async move {
            let result = async {
                let json = serde_json::to_vec(&document)?;
                let compressed = zstd::encode_all(json.as_slice(), 5)
                    .map_err(|error| StoreError::Snapshot(error.to_string()))?;
                sqlx::query!(
                    r#"INSERT INTO report_snapshots (doc_id, rev_num, snapshot_zstd)
                       VALUES ($1, $2, $3)
                       ON CONFLICT (doc_id, rev_num) DO NOTHING"#,
                    &doc_id,
                    revision.get(),
                    compressed
                )
                .execute(&pool)
                .await?;
                Ok::<(), StoreError>(())
            }
            .await;
            if let Err(error) = result {
                error!(%error, %doc_id, revision = revision.get(), "failed to persist snapshot");
            }
        });
    }
}
