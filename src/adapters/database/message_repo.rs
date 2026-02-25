use crate::adapters::database::records::MessageRecord;
use crate::domain::message::Message;
use crate::error::{AppError, Result};
use sqlx::PgConnection;
use time::{Duration, OffsetDateTime};
use uuid::Uuid;

#[derive(Clone, Debug, Default)]
pub struct MessageRepository {}

impl MessageRepository {
    #[must_use]
    pub const fn new() -> Self {
        Self {}
    }

    /// Checks which recipients exist in the database.
    ///
    /// # Errors
    /// Returns `AppError::Database` if the query fails.
    #[tracing::instrument(level = "debug", skip(self, conn))]
    pub(crate) async fn check_recipients_exist(
        &self,
        conn: &mut PgConnection,
        recipient_ids: &[Uuid],
    ) -> Result<Vec<Uuid>> {
        if recipient_ids.is_empty() {
            return Ok(Vec::new());
        }

        let rows: Vec<(Uuid,)> = sqlx::query_as("SELECT id FROM users WHERE id = ANY($1)")
            .bind(recipient_ids)
            .fetch_all(conn)
            .await
            .map_err(AppError::Database)?;

        Ok(rows.into_iter().map(|(id,)| id).collect())
    }

    /// Inserts a batch of messages.
    ///
    /// Ignores duplicate messages (based on `sender_id` and `submission_id`) via `ON CONFLICT DO NOTHING`.
    /// Returns the list of `(recipient_id, submission_id)` that were successfully inserted.
    ///
    /// # Errors
    /// Returns `AppError::Database` if the insert fails.
    #[tracing::instrument(level = "debug", skip(self, conn, messages))]
    pub(crate) async fn create_batch(
        &self,
        conn: &mut PgConnection,
        sender_id: Uuid,
        messages: Vec<(Uuid, Uuid, Vec<u8>)>,
        ttl_days: i64,
    ) -> Result<Vec<(Uuid, Uuid)>> {
        if messages.is_empty() {
            return Ok(Vec::new());
        }

        let expires_at = OffsetDateTime::now_utc() + Duration::days(ttl_days);

        let mut recipient_ids = Vec::with_capacity(messages.len());
        let mut submission_ids = Vec::with_capacity(messages.len());
        let mut contents = Vec::with_capacity(messages.len());

        for (recipient_id, submission_id, content) in messages {
            recipient_ids.push(recipient_id);
            submission_ids.push(submission_id);
            contents.push(content);
        }

        let inserted = sqlx::query_as::<_, (Uuid, Uuid)>(
            r#"
            INSERT INTO messages (sender_id, recipient_id, submission_id, content, expires_at)
            SELECT $1, u.r_id, u.s_id, u.content, $5
            FROM UNNEST($2::uuid[], $3::uuid[], $4::bytea[]) AS u(r_id, s_id, content)
            ON CONFLICT (sender_id, submission_id) DO NOTHING
            RETURNING recipient_id, submission_id
            "#,
        )
        .bind(sender_id)
        .bind(recipient_ids)
        .bind(submission_ids)
        .bind(contents)
        .bind(expires_at)
        .fetch_all(conn)
        .await
        .map_err(AppError::Database)?;

        Ok(inserted)
    }

    /// Fetches a batch of pending messages for a recipient.
    ///
    /// # Errors
    /// Returns `sqlx::Error` if the query fails.
    #[tracing::instrument(level = "debug", skip(self, conn))]
    pub(crate) async fn fetch_pending_batch(
        &self,
        conn: &mut PgConnection,
        recipient_id: Uuid,
        cursor: Option<(OffsetDateTime, Uuid)>,
        limit: i64,
    ) -> Result<Vec<Message>> {
        let messages = match cursor {
            Some((last_ts, last_id)) => {
                sqlx::query_as::<_, MessageRecord>(
                    r#"
                    SELECT id, sender_id, content, created_at
                    FROM messages
                    WHERE recipient_id = $1
                      AND expires_at > NOW()
                      AND (created_at, id) > ($2, $3)
                    ORDER BY created_at ASC, id ASC
                    LIMIT $4
                    "#,
                )
                .bind(recipient_id)
                .bind(last_ts)
                .bind(last_id)
                .bind(limit)
                .fetch_all(conn)
                .await?
            }
            None => {
                sqlx::query_as::<_, MessageRecord>(
                    r#"
                    SELECT id, sender_id, content, created_at
                    FROM messages
                    WHERE recipient_id = $1
                      AND expires_at > NOW()
                    ORDER BY created_at ASC, id ASC
                    LIMIT $2
                    "#,
                )
                .bind(recipient_id)
                .bind(limit)
                .fetch_all(conn)
                .await?
            }
        };

        Ok(messages.into_iter().map(Into::into).collect())
    }

    /// Deletes a batch of messages for a specific user.
    ///
    /// # Errors
    /// Returns `sqlx::Error` if the deletion fails.
    #[tracing::instrument(level = "debug", skip(self, conn))]
    pub(crate) async fn delete_batch(
        &self,
        conn: &mut PgConnection,
        user_id: Uuid,
        message_ids: &[Uuid],
    ) -> Result<()> {
        if message_ids.is_empty() {
            return Ok(());
        }
        sqlx::query("DELETE FROM messages WHERE id = ANY($1) AND recipient_id = $2")
            .bind(message_ids)
            .bind(user_id)
            .execute(conn)
            .await?;
        Ok(())
    }

    /// Deletes all expired messages.
    ///
    /// # Errors
    /// Returns `sqlx::Error` if the deletion fails.
    #[tracing::instrument(level = "debug", skip(self, conn), err)]
    pub async fn delete_expired(&self, conn: &mut PgConnection) -> Result<u64> {
        let result = sqlx::query("DELETE FROM messages WHERE expires_at < NOW()").execute(conn).await?;
        Ok(result.rows_affected())
    }

    /// Enforces global inbox limits by pruning the oldest messages per user.
    ///
    /// # Errors
    /// Returns `sqlx::Error` if the deletion fails.
    #[tracing::instrument(level = "debug", skip(self, conn), err)]
    pub async fn delete_global_overflow(&self, conn: &mut PgConnection, limit: i64) -> Result<u64> {
        // Deletes messages that exceed the 'limit' per recipient
        let result = sqlx::query(
            r#"
            DELETE FROM messages
            WHERE id IN (
                SELECT id FROM (
                    SELECT id, ROW_NUMBER() OVER (PARTITION BY recipient_id ORDER BY created_at DESC) as rn
                    FROM messages
                ) t WHERE t.rn > $1
            )
            "#,
        )
        .bind(limit)
        .execute(conn)
        .await?;
        Ok(result.rows_affected())
    }

    /// Deletes all messages for a specific recipient (Inbox wipe).
    ///
    /// # Errors
    /// Returns `sqlx::Error` if the deletion fails.
    #[tracing::instrument(level = "debug", skip(self, conn))]
    pub(crate) async fn delete_all_for_user(&self, conn: &mut PgConnection, user_id: Uuid) -> Result<u64> {
        let result = sqlx::query("DELETE FROM messages WHERE recipient_id = $1").bind(user_id).execute(conn).await?;
        Ok(result.rows_affected())
    }
}
