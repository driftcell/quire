use super::*;

impl KbService {
    pub async fn list_documents(&self) -> RuntimeResult<Vec<KbDocumentRecord>> {
        self.list_documents_on(None).await
    }

    pub async fn list_documents_on(
        &self,
        branch: Option<&str>,
    ) -> RuntimeResult<Vec<KbDocumentRecord>> {
        let branch = self.resolve_branch(branch).await?.name;
        let rows = sqlx::query(
            r#"SELECT doc_id, name, content, version, updated_at FROM kb_documents
               WHERE branch = $1 ORDER BY name ASC"#,
        )
        .bind(&branch)
        .fetch_all(self.store.pool())
        .await?;
        rows.iter().map(row_to_document).collect()
    }

    pub async fn upsert_document(
        &self,
        doc_id: &str,
        name: &str,
        content: &str,
    ) -> RuntimeResult<KbDocumentRecord> {
        let branch = self.active_branch().await?.name;
        self.upsert_document_on(&branch, doc_id, name, content)
            .await
    }

    pub async fn upsert_document_on(
        &self,
        branch: &str,
        doc_id: &str,
        name: &str,
        content: &str,
    ) -> RuntimeResult<KbDocumentRecord> {
        let branch_record = self.mutable_branch(branch).await?;
        sqlx::query(
            r#"INSERT INTO kb_documents (branch, doc_id, name, content, version)
               VALUES ($1, $2, $3, $4, 1)
               ON CONFLICT (branch, doc_id) DO UPDATE
               SET name = EXCLUDED.name, content = EXCLUDED.content,
                   version = kb_documents.version + 1, updated_at = NOW()"#,
        )
        .bind(&branch_record.name)
        .bind(doc_id)
        .bind(name)
        .bind(content)
        .execute(self.store.pool())
        .await?;
        self.after_kb_mutation_on(&branch_record.name, branch_record.status)
            .await?;
        let row = sqlx::query(
            r#"SELECT doc_id, name, content, version, updated_at FROM kb_documents
               WHERE branch = $1 AND doc_id = $2"#,
        )
        .bind(&branch_record.name)
        .bind(doc_id)
        .fetch_one(self.store.pool())
        .await?;
        row_to_document(&row)
    }

    pub async fn delete_document(&self, doc_id: &str) -> RuntimeResult<()> {
        let branch = self.active_branch().await?.name;
        self.delete_document_on(&branch, doc_id).await
    }

    pub async fn delete_document_on(&self, branch: &str, doc_id: &str) -> RuntimeResult<()> {
        let branch_record = self.mutable_branch(branch).await?;
        sqlx::query("DELETE FROM kb_documents WHERE branch = $1 AND doc_id = $2")
            .bind(&branch_record.name)
            .bind(doc_id)
            .execute(self.store.pool())
            .await?;
        self.after_kb_mutation_on(&branch_record.name, branch_record.status)
            .await?;
        Ok(())
    }

    // ---------------------------------------------------------------------
    // Agents
    // ---------------------------------------------------------------------
}
