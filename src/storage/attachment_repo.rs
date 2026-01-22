use crate::error::Result;
use crate::storage::DbPool;
use sqlx::Row;
use time::OffsetDateTime;
use uuid::Uuid;

#[derive(Clone)]
pub struct AttachmentRepository {
    pool: DbPool,
}

impl AttachmentRepository {
    pub fn new(pool: DbPool) -> Self {
        Self { pool }
    }

    pub async fn create(&self, id: Uuid, expires_at: OffsetDateTime) -> Result<()> {
        sqlx::query("INSERT INTO attachments (id, expires_at) VALUES ($1, $2)")
            .bind(id)
            .bind(expires_at)
            .execute(&self.pool)
            .await?;
        Ok(())
    }

    pub async fn get_expires_at(&self, id: Uuid) -> Result<Option<OffsetDateTime>> {
        let row = sqlx::query("SELECT expires_at FROM attachments WHERE id = $1")
            .bind(id)
            .fetch_optional(&self.pool)
            .await?;

        Ok(row.map(|r| r.get("expires_at")))
    }

    pub async fn delete(&self, id: Uuid) -> Result<()> {
        sqlx::query("DELETE FROM attachments WHERE id = $1").bind(id).execute(&self.pool).await?;
        Ok(())
    }

    pub async fn fetch_expired(&self, limit: i64) -> Result<Vec<Uuid>> {
        let rows = sqlx::query("SELECT id FROM attachments WHERE expires_at < NOW() LIMIT $1")
            .bind(limit)
            .fetch_all(&self.pool)
            .await?;

        Ok(rows.into_iter().map(|r| r.get("id")).collect())
    }
}
