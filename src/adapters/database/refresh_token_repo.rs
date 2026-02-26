use crate::error::{AppError, Result};
use sqlx::PgConnection;
use time::OffsetDateTime;
use uuid::Uuid;

#[derive(Clone, Debug, Default)]
pub struct RefreshTokenRepository {}

impl RefreshTokenRepository {
    #[must_use]
    pub const fn new() -> Self {
        Self {}
    }

    /// Creates a new refresh token record.
    /// Note: We store the HASH, not the raw token.
    ///
    /// # Errors
    /// Returns `AppError::Database` if the insert fails.
    #[tracing::instrument(level = "debug", skip(self, conn, token_hash), err)]
    pub(crate) async fn create(
        &self,
        conn: &mut PgConnection,
        user_id: Uuid,
        token_hash: &str,
        ttl_days: i64,
    ) -> Result<()> {
        let expires_at = OffsetDateTime::now_utc() + time::Duration::days(ttl_days);

        sqlx::query("INSERT INTO refresh_tokens (token_hash, user_id, expires_at) VALUES ($1, $2, $3)")
            .bind(token_hash)
            .bind(user_id)
            .bind(expires_at)
            .execute(conn)
            .await
            .map_err(AppError::Database)?;

        Ok(())
    }

    /// Atomically rotates a refresh token.
    /// Deletes the old token and inserts a new one only if the old one was not expired.
    /// Returns the `user_id` if successful, or None if the old token was invalid or expired.
    ///
    /// # Errors
    /// Returns `sqlx::Error` if the operation fails.
    #[tracing::instrument(level = "debug", skip(self, conn, old_hash, new_hash), err)]
    pub(crate) async fn rotate_unexpired(
        &self,
        conn: &mut PgConnection,
        old_hash: &str,
        new_hash: &str,
        ttl_days: i64,
    ) -> Result<Option<Uuid>> {
        let expires_at = OffsetDateTime::now_utc() + time::Duration::days(ttl_days);

        let user_id = sqlx::query_scalar::<_, Uuid>(
            r#"
            WITH deleted AS (
                DELETE FROM refresh_tokens
                WHERE token_hash = $1
                RETURNING user_id, expires_at
            )
            INSERT INTO refresh_tokens (token_hash, user_id, expires_at)
            SELECT $2, user_id, $3
            FROM deleted
            WHERE expires_at > NOW()
            RETURNING user_id
            "#,
        )
        .bind(old_hash)
        .bind(new_hash)
        .bind(expires_at)
        .fetch_optional(conn)
        .await?;

        Ok(user_id)
    }

    /// Revokes a specific refresh token owned by the user (Logout).
    ///
    /// # Errors
    /// Returns `AppError::Database` if the deletion fails.
    #[tracing::instrument(level = "debug", skip(self, conn, token_hash), err)]
    pub(crate) async fn delete_owned(&self, conn: &mut PgConnection, token_hash: &str, user_id: Uuid) -> Result<()> {
        sqlx::query("DELETE FROM refresh_tokens WHERE token_hash = $1 AND user_id = $2")
            .bind(token_hash)
            .bind(user_id)
            .execute(conn)
            .await
            .map_err(AppError::Database)?;
        Ok(())
    }

    /// Deletes all expired refresh tokens.
    ///
    /// # Errors
    /// Returns `AppError::Database` if the deletion fails.
    #[tracing::instrument(level = "debug", skip(self, conn), err)]
    pub async fn delete_expired(&self, conn: &mut PgConnection) -> Result<u64> {
        let result = sqlx::query("DELETE FROM refresh_tokens WHERE expires_at < NOW()")
            .execute(conn)
            .await
            .map_err(AppError::Database)?;
        Ok(result.rows_affected())
    }
}
