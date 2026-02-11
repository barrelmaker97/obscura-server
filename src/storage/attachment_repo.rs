use crate::domain::attachment::Attachment;
use crate::error::Result;
use crate::storage::records::Attachment as AttachmentRecord;
use sqlx::PgConnection;
use time::OffsetDateTime;
use uuid::Uuid;

#[derive(Clone, Default)]
pub struct AttachmentRepository {}

impl AttachmentRepository {
    pub fn new() -> Self {
        Self {}
    }

    #[tracing::instrument(level = "debug", skip(self, conn))]
    pub async fn create(&self, conn: &mut PgConnection, id: Uuid, expires_at: OffsetDateTime) -> Result<()> {
        sqlx::query("INSERT INTO attachments (id, expires_at) VALUES ($1, $2)")
            .bind(id)
            .bind(expires_at)
            .execute(conn)
            .await?;
        Ok(())
    }

    #[tracing::instrument(level = "debug", skip(self, conn))]
    pub async fn find_by_id(&self, conn: &mut PgConnection, id: Uuid) -> Result<Option<Attachment>> {
        let record = sqlx::query_as::<_, AttachmentRecord>("SELECT id, expires_at FROM attachments WHERE id = $1")
            .bind(id)
            .fetch_optional(conn)
            .await?;

        Ok(record.map(Into::into))
    }

    #[tracing::instrument(level = "debug", skip(self, conn))]
    pub async fn delete(&self, conn: &mut PgConnection, id: Uuid) -> Result<()> {
        sqlx::query("DELETE FROM attachments WHERE id = $1").bind(id).execute(conn).await?;
        Ok(())
    }

    #[tracing::instrument(level = "debug", skip(self, conn))]
    pub async fn fetch_expired(&self, conn: &mut PgConnection, limit: i64) -> Result<Vec<Uuid>> {
        let rows = sqlx::query_as::<_, AttachmentRecord>("SELECT id, expires_at FROM attachments WHERE expires_at < NOW() LIMIT $1")
            .bind(limit)
            .fetch_all(conn)
            .await?;

        Ok(rows.into_iter().map(|r| r.id).collect())
    }
}
