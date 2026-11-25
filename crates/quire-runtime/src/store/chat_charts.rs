use crate::{error::RuntimeResult, preview::ChartPreviewRecord};

use super::RuntimeStore;

impl RuntimeStore {
    pub async fn save_chat_chart(
        &self,
        session_id: &str,
        record: &ChartPreviewRecord,
    ) -> RuntimeResult<()> {
        let payload = serde_json::to_value(record)?;
        sqlx::query!(
            r#"INSERT INTO chat_charts (chart_id, session_id, payload)
               VALUES ($1, $2, $3)
               ON CONFLICT (chart_id)
               DO UPDATE SET session_id = EXCLUDED.session_id,
                             payload = EXCLUDED.payload,
                             updated_at = NOW()"#,
            record.preview_id,
            session_id,
            payload
        )
        .execute(&self.pool)
        .await?;
        Ok(())
    }

    pub async fn load_chat_chart(
        &self,
        chart_id: &str,
    ) -> RuntimeResult<Option<ChartPreviewRecord>> {
        let row = sqlx::query!(
            r#"SELECT payload FROM chat_charts WHERE chart_id = $1"#,
            chart_id.trim()
        )
        .fetch_optional(&self.pool)
        .await?;
        row.map(|r| serde_json::from_value(r.payload).map_err(Into::into))
            .transpose()
    }
}
