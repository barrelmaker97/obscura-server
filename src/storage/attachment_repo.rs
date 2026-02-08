use crate::domain::attachment::Attachment;
use crate::error::Result;
use crate::storage::records::Attachment as AttachmentRecord;
use sqlx::{Executor, Postgres};
use time::OffsetDateTime;
use uuid::Uuid;

#[derive(Clone, Default)]
pub struct AttachmentRepository {}

impl AttachmentRepository {
    pub fn new() -> Self {
        Self {}
    }

    #[tracing::instrument(level = "debug", skip(self, executor))]
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

    #[tracing::instrument(level = "debug", skip(self, executor))]
    pub async fn find_by_id<'e, E>(&self, executor: E, id: Uuid) -> Result<Option<Attachment>>
    where
        E: Executor<'e, Database = Postgres>,
    {
        let record = sqlx::query_as::<_, AttachmentRecord>("SELECT id, expires_at FROM attachments WHERE id = $1")
            .bind(id)
            .fetch_optional(executor)
            .await?;

        Ok(record.map(Into::into))
    }

    #[tracing::instrument(level = "debug", skip(self, executor))]
    pub async fn delete<'e, E>(&self, executor: E, id: Uuid) -> Result<()>
    where
        E: Executor<'e, Database = Postgres>,
    {
        sqlx::query("DELETE FROM attachments WHERE id = $1").bind(id).execute(executor).await?;
        Ok(())
    }

    #[tracing::instrument(level = "debug", skip(self, executor))]
    pub async fn fetch_expired<'e, E>(&self, executor: E, limit: i64) -> Result<Vec<Uuid>>
    where
        E: Executor<'e, Database = Postgres>,
    {
        let rows = sqlx::query_as::<_, AttachmentRecord>("SELECT id, expires_at FROM attachments WHERE expires_at < NOW() LIMIT $1")
            .bind(limit)
            .fetch_all(executor)
            .await?;

        Ok(rows.into_iter().map(|r| r.id).collect())
    }
}
