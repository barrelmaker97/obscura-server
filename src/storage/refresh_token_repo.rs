use crate::error::{AppError, Result};
use crate::storage::DbPool;
use sqlx::{Postgres, Transaction};
use time::OffsetDateTime;
use uuid::Uuid;

#[derive(Clone)]
pub struct RefreshTokenRepository {
    pool: DbPool,
}

impl RefreshTokenRepository {
    pub fn new(pool: DbPool) -> Self {
        Self { pool }
    }

    /// Creates a new refresh token record.
    /// Note: We store the HASH, not the raw token.
    pub async fn create(
        &self,
        tx: &mut Transaction<'_, Postgres>,
        user_id: Uuid,
        token_hash: &str,
        ttl_days: i64,
    ) -> Result<()> {
        let expires_at = OffsetDateTime::now_utc() + time::Duration::days(ttl_days);

        sqlx::query(
            "INSERT INTO refresh_tokens (token_hash, user_id, expires_at) VALUES ($1, $2, $3)",
        )
        .bind(token_hash)
        .bind(user_id)
        .bind(expires_at)
        .execute(&mut **tx)
        .await
        .map_err(|e| AppError::Database(e))?;

        Ok(())
    }

    /// Atomically validates and rotates a refresh token.
    /// If valid: Returns the user_id and DELETES the token (Single Usage).
    /// If invalid/expired: Returns None.
    pub async fn verify_and_rotate(&self, token_hash: &str) -> Result<Option<Uuid>> {
        let mut tx = self.pool.begin().await?;

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
        .fetch_optional(&mut *tx)
        .await
        .map_err(AppError::Database)?;

        if let Some(record) = row {
            // 2. Check Expiry
            if record.expires_at < OffsetDateTime::now_utc() {
                // Delete expired token to clean up
                sqlx::query("DELETE FROM refresh_tokens WHERE token_hash = $1")
                    .bind(token_hash)
                    .execute(&mut *tx)
                    .await?;
                tx.commit().await?;
                return Ok(None);
            }

            // 3. Delete (Rotate)
            sqlx::query("DELETE FROM refresh_tokens WHERE token_hash = $1")
                .bind(token_hash)
                .execute(&mut *tx)
                .await?;

            tx.commit().await?;
            Ok(Some(record.user_id))
        } else {
            Ok(None)
        }
    }

    /// Revokes a specific refresh token (Logout).
    pub async fn delete(&self, token_hash: &str) -> Result<()> {
        sqlx::query("DELETE FROM refresh_tokens WHERE token_hash = $1")
            .bind(token_hash)
            .execute(&self.pool)
            .await
            .map_err(AppError::Database)?;
        Ok(())
    }
    
    /// Revokes all tokens for a user (e.g., security breach).
    pub async fn delete_all_for_user(&self, user_id: Uuid) -> Result<()> {
         sqlx::query("DELETE FROM refresh_tokens WHERE user_id = $1")
            .bind(user_id)
            .execute(&self.pool)
            .await
            .map_err(AppError::Database)?;
        Ok(())
    }
}
