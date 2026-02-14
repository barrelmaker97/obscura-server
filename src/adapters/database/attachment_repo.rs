use crate::domain::attachment::Attachment;
use crate::error::Result;
use crate::adapters::database::records::AttachmentRecord;
use sqlx::PgConnection;
use time::OffsetDateTime;
use uuid::Uuid;

#[derive(Clone, Debug, Default)]
pub struct AttachmentRepository {}

impl AttachmentRepository {
    #[must_use]
    pub const fn new() -> Self {
        Self {}
    }

    /// Records a new attachment in the database.
    ///
    /// # Errors
    /// Returns `sqlx::Error` if the insert fails.
    #[tracing::instrument(level = "debug", skip(self, conn))]
    pub(crate) async fn create(&self, conn: &mut PgConnection, id: Uuid, expires_at: OffsetDateTime) -> Result<()> {
        sqlx::query("INSERT INTO attachments (id, expires_at) VALUES ($1, $2)")
            .bind(id)
            .bind(expires_at)
            .execute(conn)
            .await?;
        Ok(())
    }

    /// Finds an attachment by its ID.
    ///
    /// # Errors
    /// Returns `sqlx::Error` if the query fails.
    #[tracing::instrument(level = "debug", skip(self, conn))]
    pub(crate) async fn find_by_id(&self, conn: &mut PgConnection, id: Uuid) -> Result<Option<Attachment>> {
        let record = sqlx::query_as::<_, AttachmentRecord>("SELECT id, expires_at FROM attachments WHERE id = $1")
            .bind(id)
            .fetch_optional(conn)
            .await?;

        Ok(record.map(Into::into))
    }

    /// Deletes an attachment record.
    ///
    /// # Errors
    /// Returns `sqlx::Error` if the deletion fails.
    #[tracing::instrument(level = "debug", skip(self, conn))]
    pub(crate) async fn delete(&self, conn: &mut PgConnection, id: Uuid) -> Result<()> {
        sqlx::query("DELETE FROM attachments WHERE id = $1").bind(id).execute(conn).await?;
        Ok(())
    }

    /// Fetches a batch of expired attachment IDs.
    ///
    /// # Errors
    /// Returns `sqlx::Error` if the query fails.
    #[tracing::instrument(level = "debug", skip(self, conn))]
    pub(crate) async fn fetch_expired(&self, conn: &mut PgConnection, limit: i64) -> Result<Vec<Uuid>> {
        let rows = sqlx::query_as::<_, AttachmentRecord>(
            "SELECT id, expires_at FROM attachments WHERE expires_at < NOW() LIMIT $1",
        )
        .bind(limit)
        .fetch_all(conn)
        .await?;

        Ok(rows.into_iter().map(|r| r.id).collect())
    }
}
