use crate::core::message::Message;
use crate::error::{AppError, Result};
use sqlx::PgPool;
use time::{Duration, OffsetDateTime};
use uuid::Uuid;

#[derive(Clone)]
pub struct MessageRepository {
    pool: PgPool,
}

impl MessageRepository {
    pub fn new(pool: PgPool) -> Self {
        Self { pool }
    }

    pub async fn create(&self, sender_id: Uuid, recipient_id: Uuid, content: Vec<u8>, ttl_days: i64) -> Result<()> {
        let expires_at = OffsetDateTime::now_utc() + Duration::days(ttl_days);

        let result = sqlx::query(
            r#"
            INSERT INTO messages (sender_id, recipient_id, content, expires_at)
            VALUES ($1, $2, $3, $4)
            "#,
        )
        .bind(sender_id)
        .bind(recipient_id)
        .bind(content)
        .bind(expires_at)
        .execute(&self.pool)
        .await;

        match result {
            Ok(_) => Ok(()),
            Err(sqlx::Error::Database(e)) if e.code().as_deref() == Some("23503") => {
                // Foreign key violation: recipient_id does not exist
                Err(AppError::NotFound)
            }
            Err(e) => Err(AppError::Database(e)),
        }
    }

    pub async fn fetch_pending_batch(
        &self,
        recipient_id: Uuid,
        cursor: Option<(OffsetDateTime, Uuid)>,
        limit: i64,
    ) -> Result<Vec<Message>> {
        let messages = match cursor {
            Some((last_ts, last_id)) => {
                sqlx::query_as::<_, Message>(
                    r#"
                    SELECT id, sender_id, recipient_id, content, created_at, expires_at
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
                .fetch_all(&self.pool)
                .await?
            }
            None => {
                sqlx::query_as::<_, Message>(
                    r#"
                    SELECT id, sender_id, recipient_id, content, created_at, expires_at
                    FROM messages
                    WHERE recipient_id = $1 
                      AND expires_at > NOW()
                    ORDER BY created_at ASC, id ASC
                    LIMIT $2
                    "#,
                )
                .bind(recipient_id)
                .bind(limit)
                .fetch_all(&self.pool)
                .await?
            }
        };

        Ok(messages)
    }

    pub async fn delete_batch(&self, message_ids: &[Uuid]) -> Result<()> {
        if message_ids.is_empty() {
            return Ok(());
        }
        sqlx::query("DELETE FROM messages WHERE id = ANY($1)")
            .bind(message_ids)
            .execute(&self.pool)
            .await?;
        Ok(())
    }

    pub async fn delete_expired(&self) -> Result<u64> {
        let result = sqlx::query("DELETE FROM messages WHERE expires_at < NOW()").execute(&self.pool).await?;
        Ok(result.rows_affected())
    }

    pub async fn delete_global_overflow(&self, limit: i64) -> Result<u64> {
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
        .execute(&self.pool)
        .await?;
        Ok(result.rows_affected())
    }

    pub async fn delete_all_for_user<'e, E>(&self, executor: E, user_id: Uuid) -> Result<u64>
    where
        E: sqlx::Executor<'e, Database = sqlx::Postgres>,
    {
        let result =
            sqlx::query("DELETE FROM messages WHERE recipient_id = $1").bind(user_id).execute(executor).await?;
        Ok(result.rows_affected())
    }
}
