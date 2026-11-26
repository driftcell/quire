use quire_protocol::short_id;
use sqlx::Row;

use crate::error::{RuntimeError, RuntimeResult};

use super::RuntimeStore;

impl RuntimeStore {
    pub async fn ensure_external_session_for_report(
        &self,
        requested_session_id: Option<&str>,
        user_id: Option<&str>,
        is_admin: bool,
        title: &str,
        doc_id: Option<&str>,
    ) -> RuntimeResult<String> {
        let title = title.trim();
        let effective_title = if title.is_empty() {
            "外部代理报告"
        } else {
            title
        };
        if let Some(existing_id) = requested_session_id
            .map(str::trim)
            .filter(|s| !s.is_empty())
        {
            let existing_row =
                sqlx::query("SELECT user_id FROM runtime.sessions WHERE session_id = $1")
                    .bind(existing_id)
                    .fetch_optional(&self.pool)
                    .await?;
            if let Some(row) = existing_row {
                let existing_owner: Option<String> = row.try_get("user_id")?;
                if !is_admin
                    && let Some(caller_uid) = user_id
                    && existing_owner.as_deref() != Some(caller_uid)
                {
                    return Err(RuntimeError::Forbidden(format!(
                        "session `{existing_id}` belongs to another user"
                    )));
                }
                if let Some(doc_id) = doc_id {
                    let updated = sqlx::query(
                        r#"UPDATE runtime.sessions
                           SET doc_id = $2,
                               linked_doc_id = $2,
                               user_id = COALESCE(user_id, $3),
                               updated_at = NOW()
                           WHERE session_id = $1
                             AND ($4 OR $3 IS NULL OR user_id = $3)"#,
                    )
                    .bind(existing_id)
                    .bind(doc_id)
                    .bind(user_id)
                    .bind(is_admin)
                    .execute(&self.pool)
                    .await?;
                    if updated.rows_affected() == 0 {
                        return Err(RuntimeError::Forbidden(format!(
                            "session `{existing_id}` belongs to another user"
                        )));
                    }
                }
                if let Some(doc_id) = doc_id {
                    sqlx::query(
                        r#"UPDATE report.documents d
                           SET owner_user_id = COALESCE(d.owner_user_id, $2, s.user_id)
                           FROM runtime.sessions s
                           WHERE d.doc_id = $1
                             AND s.session_id = $3"#,
                    )
                    .bind(doc_id)
                    .bind(user_id)
                    .bind(existing_id)
                    .execute(&self.pool)
                    .await?;
                }
                return Ok(existing_id.to_string());
            }
        }
        let session_id = requested_session_id
            .map(str::trim)
            .filter(|s| !s.is_empty())
            .map(ToString::to_string)
            .unwrap_or_else(|| short_id("sess"));
        let branch: String = sqlx::query_scalar(
            "SELECT name FROM kb.branches WHERE status = 'active' ORDER BY name ASC LIMIT 1",
        )
        .fetch_optional(&self.pool)
        .await?
        .unwrap_or_else(|| "main".to_string());
        let upserted = sqlx::query(
            r#"INSERT INTO runtime.sessions
               (session_id, user_id, title, doc_id, linked_doc_id, kb_branch, status, active_run_status, created_via)
               VALUES ($1, $2, $3, $4, $4, $5, 'active', 'idle', 'mcp')
               ON CONFLICT (session_id) DO UPDATE
               SET doc_id = COALESCE(EXCLUDED.doc_id, runtime.sessions.doc_id),
                   linked_doc_id = COALESCE(EXCLUDED.linked_doc_id, runtime.sessions.linked_doc_id),
                   user_id = COALESCE(runtime.sessions.user_id, EXCLUDED.user_id),
                   updated_at = NOW()
               WHERE $6
                  OR EXCLUDED.user_id IS NULL
                  OR runtime.sessions.user_id = EXCLUDED.user_id"#,
        )
        .bind(&session_id)
        .bind(user_id)
        .bind(effective_title)
        .bind(doc_id)
        .bind(&branch)
        .bind(is_admin)
        .execute(&self.pool)
        .await?;
        if upserted.rows_affected() == 0 {
            return Err(RuntimeError::Forbidden(format!(
                "session `{session_id}` belongs to another user"
            )));
        }
        if let Some(doc_id) = doc_id {
            sqlx::query(
                r#"UPDATE report.documents d
                   SET owner_user_id = COALESCE(d.owner_user_id, $2, s.user_id)
                   FROM runtime.sessions s
                   WHERE d.doc_id = $1
                     AND s.session_id = $3"#,
            )
            .bind(doc_id)
            .bind(user_id)
            .bind(&session_id)
            .execute(&self.pool)
            .await?;
        }
        Ok(session_id)
    }

    pub async fn authorize_document_access(
        &self,
        doc_id: &str,
        doc_session_id: Option<&str>,
        user_id: Option<&str>,
        caller_session_id: Option<&str>,
        is_admin: bool,
    ) -> RuntimeResult<()> {
        if is_admin {
            return Ok(());
        }
        let doc_row =
            sqlx::query("SELECT session_id, owner_user_id FROM report.documents WHERE doc_id = $1")
                .bind(doc_id)
                .fetch_optional(&self.pool)
                .await?;
        let effective_doc_session = match doc_session_id {
            Some(sid) => Some(sid.to_string()),
            None => doc_row.as_ref().and_then(|row| {
                row.try_get::<Option<String>, _>("session_id")
                    .ok()
                    .flatten()
            }),
        };
        let authoritative_owner = doc_row.as_ref().and_then(|row| {
            row.try_get::<Option<String>, _>("owner_user_id")
                .ok()
                .flatten()
        });
        let doc_session_owner = if let Some(doc_sid) = effective_doc_session.as_deref() {
            sqlx::query_scalar::<_, Option<String>>(
                "SELECT user_id FROM runtime.sessions WHERE session_id = $1",
            )
            .bind(doc_sid)
            .fetch_optional(&self.pool)
            .await?
            .flatten()
        } else {
            None
        };
        let effective_owner = authoritative_owner.or(doc_session_owner);

        if let Some(caller_uid) = user_id {
            if let Some(req_sid) = caller_session_id {
                let req_owner: Option<Option<String>> = sqlx::query_scalar(
                    "SELECT user_id FROM runtime.sessions WHERE session_id = $1",
                )
                .bind(req_sid)
                .fetch_optional(&self.pool)
                .await?;
                if let Some(owner) = req_owner
                    && owner.as_deref() != Some(caller_uid)
                {
                    return Err(RuntimeError::Forbidden(format!(
                        "session `{req_sid}` belongs to another user"
                    )));
                }
            }
            if effective_owner.as_deref() == Some(caller_uid) {
                return Ok(());
            }
            return Err(RuntimeError::Forbidden(format!(
                "you do not have permission to access report `{doc_id}`"
            )));
        }

        if let Some(caller_sid) = caller_session_id {
            let caller_uid: Option<String> =
                sqlx::query_scalar("SELECT user_id FROM runtime.sessions WHERE session_id = $1")
                    .bind(caller_sid)
                    .fetch_optional(&self.pool)
                    .await?
                    .flatten();

            if let Some(owner_uid) = effective_owner.as_deref() {
                if caller_uid.as_deref() == Some(owner_uid) {
                    return Ok(());
                }
                return Err(RuntimeError::Forbidden(format!(
                    "session `{caller_sid}` does not have permission to access report `{doc_id}`"
                )));
            }

            if effective_doc_session.as_deref() == Some(caller_sid) {
                return Ok(());
            }
            return Err(RuntimeError::Forbidden(format!(
                "session `{caller_sid}` does not have permission to access report `{doc_id}`"
            )));
        }

        Err(RuntimeError::Forbidden(
            "authentication required to access report".into(),
        ))
    }

    pub async fn authorize_session_access(
        &self,
        session_id: &str,
        user_id: Option<&str>,
        is_admin: bool,
    ) -> RuntimeResult<()> {
        if is_admin {
            return Ok(());
        }
        let Some(caller_uid) = user_id else {
            return Err(RuntimeError::Forbidden(
                "authentication required to access session".into(),
            ));
        };
        let owner = self.get_session_owner(session_id).await?;
        if owner.as_deref() == Some(caller_uid) {
            return Ok(());
        }
        Err(RuntimeError::Forbidden(
            "you do not have permission to access this session".into(),
        ))
    }

    pub async fn accessible_report_scope(
        &self,
        user_id: Option<&str>,
        caller_session_id: Option<&str>,
        is_admin: bool,
    ) -> RuntimeResult<Option<(Vec<String>, Vec<String>)>> {
        if is_admin {
            return Ok(None);
        }
        let mut effective_uid = user_id.map(ToString::to_string);
        let sessions = Vec::new();
        let mut docs = Vec::new();

        if effective_uid.is_none()
            && let Some(sid) = caller_session_id
        {
            effective_uid = sqlx::query_scalar::<_, Option<String>>(
                "SELECT user_id FROM runtime.sessions WHERE session_id = $1",
            )
            .bind(sid)
            .fetch_optional(&self.pool)
            .await?
            .flatten();
            if effective_uid.is_none() {
                let doc_ids: Vec<String> = sqlx::query_scalar(
                    r#"SELECT doc_id
                       FROM report.documents
                       WHERE owner_user_id IS NULL AND session_id = $1"#,
                )
                .bind(sid)
                .fetch_all(&self.pool)
                .await?;
                docs.extend(doc_ids);
            }
        }

        if let Some(uid) = effective_uid.as_deref() {
            let doc_ids: Vec<String> = sqlx::query_scalar(
                r#"SELECT d.doc_id
                   FROM report.documents d
                   LEFT JOIN runtime.sessions s ON s.session_id = d.session_id
                   WHERE d.owner_user_id = $1
                      OR (d.owner_user_id IS NULL AND s.user_id = $1)"#,
            )
            .bind(uid)
            .fetch_all(&self.pool)
            .await?;
            docs.extend(doc_ids);
        }
        Ok(Some((sessions, docs)))
    }
}
