use crate::core::message::Message;
use crate::error::Result;
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

    pub async fn create(
        &self,
        sender_id: Uuid,
        recipient_id: Uuid,
        content: Vec<u8>,
        ttl_days: i64,
    ) -> Result<()> {
        let expires_at = OffsetDateTime::now_utc() + Duration::days(ttl_days);

        sqlx::query(
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
        .await?;

        Ok(())
    }

    pub async fn fetch_pending(&self, recipient_id: Uuid) -> Result<Vec<Message>> {
        let messages = sqlx::query_as::<_, Message>(
            r#"
            SELECT id, sender_id, recipient_id, content, created_at, expires_at
            FROM messages
            WHERE recipient_id = $1 AND expires_at > NOW()
            ORDER BY created_at ASC
            "#,
        )
        .bind(recipient_id)
        .fetch_all(&self.pool)
        .await?;

        Ok(messages)
    }

    pub async fn delete(&self, message_id: Uuid) -> Result<()> {
        sqlx::query("DELETE FROM messages WHERE id = $1")
            .bind(message_id)
            .execute(&self.pool)
            .await?;
        Ok(())
    }

    pub async fn delete_expired(&self) -> Result<u64> {
        let result = sqlx::query("DELETE FROM messages WHERE expires_at < NOW()")
            .execute(&self.pool)
            .await?;
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
            "#
        )
        .bind(limit)
        .execute(&self.pool)
        .await?;
        Ok(result.rows_affected())
    }

    pub async fn delete_all_for_user(&self, user_id: Uuid) -> Result<u64> {
        let result = sqlx::query("DELETE FROM messages WHERE recipient_id = $1")
            .bind(user_id)
            .execute(&self.pool)
            .await?;
        Ok(result.rows_affected())
    }
}
