use crate::error::Result;
use sqlx::{Executor, Postgres, Row};
use time::OffsetDateTime;
use uuid::Uuid;

#[derive(Clone, Default)]
pub struct AttachmentRepository {}

impl AttachmentRepository {
    pub fn new() -> Self {
        Self {}
    }

    pub async fn create<'e, E>(&self, executor: E, id: Uuid, expires_at: OffsetDateTime) -> Result<()>
    where
        E: Executor<'e, Database = Postgres>,
    {
        sqlx::query("INSERT INTO attachments (id, expires_at) VALUES ($1, $2)")
            .bind(id)
            .bind(expires_at)
            .execute(executor)
            .await?;
        Ok(())
    }

    pub async fn get_expires_at<'e, E>(&self, executor: E, id: Uuid) -> Result<Option<OffsetDateTime>>
    where
        E: Executor<'e, Database = Postgres>,
    {
        let row = sqlx::query("SELECT expires_at FROM attachments WHERE id = $1")
            .bind(id)
            .fetch_optional(executor)
            .await?;

        Ok(row.map(|r| r.get("expires_at")))
    }

    pub async fn delete<'e, E>(&self, executor: E, id: Uuid) -> Result<()>
    where
        E: Executor<'e, Database = Postgres>,
    {
        sqlx::query("DELETE FROM attachments WHERE id = $1").bind(id).execute(executor).await?;
        Ok(())
    }

    pub async fn fetch_expired<'e, E>(&self, executor: E, limit: i64) -> Result<Vec<Uuid>>
    where
        E: Executor<'e, Database = Postgres>,
    {
        let rows = sqlx::query("SELECT id FROM attachments WHERE expires_at < NOW() LIMIT $1")
            .bind(limit)
            .fetch_all(executor)
            .await?;

        Ok(rows.into_iter().map(|r| r.get("id")).collect())
    }
}
