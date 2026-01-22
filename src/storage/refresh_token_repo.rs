use crate::error::{AppError, Result};
use sqlx::{Executor, PgConnection, Postgres};
use time::OffsetDateTime;
use uuid::Uuid;

#[derive(Clone, Default)]
pub struct RefreshTokenRepository {}

impl RefreshTokenRepository {
    pub fn new() -> Self {
        Self {}
    }

    /// Creates a new refresh token record.
    /// Note: We store the HASH, not the raw token.
    pub async fn create<'e, E>(&self, executor: E, user_id: Uuid, token_hash: &str, ttl_days: i64) -> Result<()>
    where
        E: Executor<'e, Database = Postgres>,
    {
        let expires_at = OffsetDateTime::now_utc() + time::Duration::days(ttl_days);

        sqlx::query("INSERT INTO refresh_tokens (token_hash, user_id, expires_at) VALUES ($1, $2, $3)")
            .bind(token_hash)
            .bind(user_id)
            .bind(expires_at)
            .execute(executor)
            .await
            .map_err(AppError::Database)?;

        Ok(())
    }

    /// Atomically validates and consumes a refresh token (Step 1 of Rotation).
    /// If valid: Returns the user_id and DELETES the token from the DB.
    /// If invalid/expired: Returns None.
    /// The caller MUST commit the transaction.
    pub async fn verify_and_consume(&self, executor: &mut PgConnection, token_hash: &str) -> Result<Option<Uuid>> {
        #[derive(sqlx::FromRow)]
        struct TokenRecord {
            user_id: Uuid,
            expires_at: OffsetDateTime,
        }

        // 1. Fetch and Lock
        let row: Option<TokenRecord> = sqlx::query_as(
            r#"
            SELECT user_id, expires_at 
            FROM refresh_tokens 
            WHERE token_hash = $1 
            FOR UPDATE SKIP LOCKED
            "#,
        )
        .bind(token_hash)
        .fetch_optional(&mut *executor)
        .await
        .map_err(AppError::Database)?;

        if let Some(record) = row {
            // 2. Check Expiry
            if record.expires_at < OffsetDateTime::now_utc() {
                // Delete expired token to clean up
                sqlx::query("DELETE FROM refresh_tokens WHERE token_hash = $1")
                    .bind(token_hash)
                    .execute(&mut *executor)
                    .await?;
                return Ok(None);
            }

            // 3. Delete (Consume)
            sqlx::query("DELETE FROM refresh_tokens WHERE token_hash = $1")
                .bind(token_hash)
                .execute(&mut *executor)
                .await?;

            Ok(Some(record.user_id))
        } else {
            Ok(None)
        }
    }

    /// Revokes a specific refresh token owned by the user (Logout).
    pub async fn delete_owned<'e, E>(&self, executor: E, token_hash: &str, user_id: Uuid) -> Result<()>
    where
        E: Executor<'e, Database = Postgres>,
    {
        sqlx::query("DELETE FROM refresh_tokens WHERE token_hash = $1 AND user_id = $2")
            .bind(token_hash)
            .bind(user_id)
            .execute(executor)
            .await
            .map_err(AppError::Database)?;
        Ok(())
    }
}
