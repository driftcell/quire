use sqlx::Row;

use crate::{
    error::{RuntimeError, RuntimeResult},
    preview::ChartPreviewRecord,
};

use super::RuntimeStore;

impl RuntimeStore {
    pub async fn save_chat_chart(
        &self,
        session_id: &str,
        record: &ChartPreviewRecord,
    ) -> RuntimeResult<()> {
        let payload = serde_json::to_value(record)?;
        sqlx::query(
            r#"INSERT INTO runtime.chat_charts (session_id, chart_id, payload)
               VALUES ($1, $2, $3)
               ON CONFLICT (session_id, chart_id)
               DO UPDATE SET payload = EXCLUDED.payload,
                             updated_at = NOW()"#,
        )
        .bind(session_id.trim())
        .bind(record.preview_id.trim())
        .bind(payload)
        .execute(&self.pool)
        .await?;
        Ok(())
    }

    pub async fn load_chat_chart_for_session(
        &self,
        session_id: &str,
        chart_id: &str,
    ) -> RuntimeResult<Option<ChartPreviewRecord>> {
        let payload: Option<serde_json::Value> = sqlx::query_scalar(
            r#"SELECT payload
               FROM runtime.chat_charts
               WHERE session_id = $1 AND chart_id = $2"#,
        )
        .bind(session_id.trim())
        .bind(chart_id.trim())
        .fetch_optional(&self.pool)
        .await?;
        payload
            .map(|value| serde_json::from_value(value).map_err(Into::into))
            .transpose()
    }

    pub async fn load_chat_chart_for_user(
        &self,
        chart_id: &str,
        user_id: &str,
        is_admin: bool,
    ) -> RuntimeResult<Option<ChartPreviewRecord>> {
        let rows = sqlx::query(
            r#"SELECT c.payload, s.user_id
               FROM runtime.chat_charts c
               JOIN runtime.sessions s ON s.session_id = c.session_id
               WHERE c.chart_id = $1
               ORDER BY c.updated_at DESC"#,
        )
        .bind(chart_id.trim())
        .fetch_all(&self.pool)
        .await?;
        if rows.is_empty() {
            return Ok(None);
        }
        for row in &rows {
            let owner: Option<String> = row.try_get("user_id")?;
            if is_admin || owner.as_deref() == Some(user_id) || owner.is_none() {
                let value: serde_json::Value = row.try_get("payload")?;
                return Ok(Some(serde_json::from_value(value)?));
            }
        }
        Err(RuntimeError::Forbidden(
            "you do not have permission to access this session chart".into(),
        ))
    }

    pub async fn load_chat_chart(
        &self,
        chart_id: &str,
    ) -> RuntimeResult<Option<ChartPreviewRecord>> {
        let payload: Option<serde_json::Value> = sqlx::query_scalar(
            r#"SELECT payload
               FROM runtime.chat_charts
               WHERE chart_id = $1
               ORDER BY updated_at DESC
               LIMIT 1"#,
        )
        .bind(chart_id.trim())
        .fetch_optional(&self.pool)
        .await?;
        payload
            .map(|value| serde_json::from_value(value).map_err(Into::into))
            .transpose()
    }
}
