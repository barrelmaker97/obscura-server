use crate::domain::message::Message;
use crate::error::{AppError, Result};
use crate::storage::models::MessageRecord;
use sqlx::{Executor, Postgres};
use time::{Duration, OffsetDateTime};
use uuid::Uuid;

#[derive(Clone, Default)]
pub struct MessageRepository {}

impl MessageRepository {
    pub fn new() -> Self {
        Self {}
    }

    #[tracing::instrument(level = "debug", skip(self, executor, content))]
    pub async fn create<'e, E>(
        &self,
        executor: E,
        sender_id: Uuid,
        recipient_id: Uuid,
        message_type: i32,
        content: Vec<u8>,
        ttl_days: i64,
    ) -> Result<Message>
    where
        E: Executor<'e, Database = Postgres>,
    {
        let expires_at = OffsetDateTime::now_utc() + Duration::days(ttl_days);

        let result = sqlx::query_as::<_, MessageRecord>(
            r#"
            INSERT INTO messages (sender_id, recipient_id, message_type, content, expires_at)
            VALUES ($1, $2, $3, $4, $5)
            RETURNING id, sender_id, recipient_id, message_type, content, created_at, expires_at
            "#,
        )
        .bind(sender_id)
        .bind(recipient_id)
        .bind(message_type)
        .bind(content)
        .bind(expires_at)
        .fetch_one(executor)
        .await;

        match result {
            Ok(record) => Ok(record.into()),
            Err(sqlx::Error::Database(e)) if e.code().as_deref() == Some("23503") => {
                // Foreign key violation: recipient_id does not exist
                Err(AppError::NotFound)
            }
            Err(e) => Err(AppError::Database(e)),
        }
    }

    #[tracing::instrument(level = "debug", skip(self, executor))]
    pub async fn find_by_id<'e, E>(&self, executor: E, id: Uuid) -> Result<Option<Message>>
    where
        E: Executor<'e, Database = Postgres>,
    {
        let record = sqlx::query_as::<_, MessageRecord>(
            r#"
            SELECT id, sender_id, recipient_id, message_type, content, created_at, expires_at
            FROM messages
            WHERE id = $1
            "#,
        )
        .bind(id)
        .fetch_optional(executor)
        .await?;

        Ok(record.map(Into::into))
    }

    #[tracing::instrument(level = "debug", skip(self, executor))]
    pub async fn fetch_pending_batch<'e, E>(
        &self,
        executor: E,
        recipient_id: Uuid,
        cursor: Option<(OffsetDateTime, Uuid)>,
        limit: i64,
    ) -> Result<Vec<Message>>
    where
        E: Executor<'e, Database = Postgres>,
    {
        let messages = match cursor {
            Some((last_ts, last_id)) => {
                sqlx::query_as::<_, MessageRecord>(
                    r#"
                    SELECT id, sender_id, recipient_id, message_type, content, created_at, expires_at
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
                .fetch_all(executor)
                .await?
            }
            None => {
                sqlx::query_as::<_, MessageRecord>(
                    r#"
                    SELECT id, sender_id, recipient_id, message_type, content, created_at, expires_at
                    FROM messages
                    WHERE recipient_id = $1
                      AND expires_at > NOW()
                    ORDER BY created_at ASC, id ASC
                    LIMIT $2
                    "#,
                )
                .bind(recipient_id)
                .bind(limit)
                .fetch_all(executor)
                .await?
            }
        };

        Ok(messages.into_iter().map(Into::into).collect())
    }

    #[tracing::instrument(level = "debug", skip(self, executor))]
    pub async fn delete_batch<'e, E>(&self, executor: E, message_ids: &[Uuid]) -> Result<()>
    where
        E: Executor<'e, Database = Postgres>,
    {
        if message_ids.is_empty() {
            return Ok(());
        }
        sqlx::query("DELETE FROM messages WHERE id = ANY($1)").bind(message_ids).execute(executor).await?;
        Ok(())
    }

    #[tracing::instrument(level = "debug", skip(self, executor))]
    pub async fn delete_expired<'e, E>(&self, executor: E) -> Result<u64>
    where
        E: Executor<'e, Database = Postgres>,
    {
        let result = sqlx::query("DELETE FROM messages WHERE expires_at < NOW()").execute(executor).await?;
        Ok(result.rows_affected())
    }

    #[tracing::instrument(level = "debug", skip(self, executor))]
    pub async fn delete_global_overflow<'e, E>(&self, executor: E, limit: i64) -> Result<u64>
    where
        E: Executor<'e, Database = Postgres>,
    {
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
        .execute(executor)
        .await?;
        Ok(result.rows_affected())
    }

    #[tracing::instrument(level = "debug", skip(self, executor))]
    pub async fn delete_all_for_user<'e, E>(&self, executor: E, user_id: Uuid) -> Result<u64>
    where
        E: sqlx::Executor<'e, Database = sqlx::Postgres>,
    {
        let result =
            sqlx::query("DELETE FROM messages WHERE recipient_id = $1").bind(user_id).execute(executor).await?;
        Ok(result.rows_affected())
    }
}