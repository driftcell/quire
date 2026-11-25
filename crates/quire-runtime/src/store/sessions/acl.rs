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
        let effective_doc_session = match doc_session_id {
            Some(sid) => Some(sid.to_string()),
            None => sqlx::query_scalar::<_, Option<String>>(
                "SELECT session_id FROM report.documents WHERE doc_id = $1",
            )
            .bind(doc_id)
            .fetch_optional(&self.pool)
            .await?
            .flatten(),
        };
        let owner_rows = sqlx::query(
            r#"SELECT session_id, user_id
               FROM runtime.sessions
               WHERE ($1::text IS NOT NULL AND session_id = $1)
                  OR doc_id = $2
                  OR linked_doc_id = $2"#,
        )
        .bind(effective_doc_session.as_deref())
        .bind(doc_id)
        .fetch_all(&self.pool)
        .await?;

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
            let owns_doc = owner_rows.iter().any(|row| {
                row.try_get::<Option<String>, _>("user_id")
                    .ok()
                    .flatten()
                    .as_deref()
                    == Some(caller_uid)
            });
            if owns_doc {
                return Ok(());
            }
            return Err(RuntimeError::Forbidden(format!(
                "you do not have permission to access report `{doc_id}`"
            )));
        }

        if let Some(caller_sid) = caller_session_id {
            let caller_row = sqlx::query(
                "SELECT user_id, doc_id, linked_doc_id FROM runtime.sessions WHERE session_id = $1",
            )
            .bind(caller_sid)
            .fetch_optional(&self.pool)
            .await?;

            if effective_doc_session.as_deref() == Some(caller_sid)
                || owner_rows.iter().any(|row| {
                    row.try_get::<String, _>("session_id").ok().as_deref() == Some(caller_sid)
                })
            {
                return Ok(());
            }
            if let Some(caller_row) = caller_row {
                let bound_doc: Option<String> = caller_row.try_get("doc_id")?;
                let linked_doc: Option<String> = caller_row.try_get("linked_doc_id")?;
                if bound_doc.as_deref() == Some(doc_id) || linked_doc.as_deref() == Some(doc_id) {
                    return Ok(());
                }
                let caller_uid: Option<String> = caller_row.try_get("user_id")?;
                if let Some(caller_uid) = caller_uid.as_deref() {
                    let same_user = owner_rows.iter().any(|row| {
                        row.try_get::<Option<String>, _>("user_id")
                            .ok()
                            .flatten()
                            .as_deref()
                            == Some(caller_uid)
                    });
                    if same_user {
                        return Ok(());
                    }
                }
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
        let mut sessions = Vec::new();
        let mut docs = Vec::new();

        if effective_uid.is_none()
            && let Some(sid) = caller_session_id
        {
            sessions.push(sid.to_string());
            if let Some(row) = sqlx::query(
                "SELECT user_id, doc_id, linked_doc_id FROM runtime.sessions WHERE session_id = $1",
            )
            .bind(sid)
            .fetch_optional(&self.pool)
            .await?
            {
                effective_uid = row.try_get("user_id")?;
                if let Some(d) = row.try_get::<Option<String>, _>("doc_id")? {
                    docs.push(d);
                }
                if let Some(d) = row.try_get::<Option<String>, _>("linked_doc_id")? {
                    docs.push(d);
                }
            }
        }

        if let Some(uid) = effective_uid.as_deref() {
            let rows = sqlx::query(
                "SELECT session_id, doc_id, linked_doc_id FROM runtime.sessions WHERE user_id = $1",
            )
            .bind(uid)
            .fetch_all(&self.pool)
            .await?;
            for row in rows {
                let sid: String = row.try_get("session_id")?;
                if !sessions.contains(&sid) {
                    sessions.push(sid);
                }
                if let Some(d) = row.try_get::<Option<String>, _>("doc_id")?
                    && !docs.contains(&d)
                {
                    docs.push(d);
                }
                if let Some(d) = row.try_get::<Option<String>, _>("linked_doc_id")?
                    && !docs.contains(&d)
                {
                    docs.push(d);
                }
            }
        }
        Ok(Some((sessions, docs)))
    }
}
