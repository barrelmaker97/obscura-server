use crate::domain::auth::RefreshToken;
use crate::error::{AppError, Result};
use crate::storage::records::RefreshToken as RefreshTokenRecord;
use sqlx::PgConnection;
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
    #[tracing::instrument(level = "debug", skip(self, conn, token_hash))]
    pub async fn create(&self, conn: &mut PgConnection, user_id: Uuid, token_hash: &str, ttl_days: i64) -> Result<()> {
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

    /// Atomically validates and consumes a refresh token (Step 1 of Rotation).
    /// If valid: Returns the user_id and DELETES the token from the DB.
    /// If invalid/expired: Returns None.
    /// The caller MUST commit the transaction.
    #[tracing::instrument(level = "debug", skip(self, conn, token_hash), fields(user_id = tracing::field::Empty))]
    pub async fn verify_and_consume(&self, conn: &mut PgConnection, token_hash: &str) -> Result<Option<Uuid>> {
        // 1. Fetch and Lock
        let record: Option<RefreshTokenRecord> = sqlx::query_as(
            r#"
            SELECT token_hash, user_id, expires_at, created_at
            FROM refresh_tokens 
            WHERE token_hash = $1 
            FOR UPDATE SKIP LOCKED
            "#,
        )
        .bind(token_hash)
        .fetch_optional(&mut *conn)
        .await
        .map_err(AppError::Database)?;

        if let Some(record) = record {
            let token: RefreshToken = record.into();
            tracing::Span::current().record("user_id", tracing::field::display(token.user_id));

            // 2. Check Expiry using domain logic
            if token.is_expired_at(OffsetDateTime::now_utc()) {
                tracing::warn!("Refresh token expired during rotation attempt");
                // Delete expired token to clean up
                sqlx::query("DELETE FROM refresh_tokens WHERE token_hash = $1")
                    .bind(token_hash)
                    .execute(&mut *conn)
                    .await?;
                return Ok(None);
            }

            // 3. Delete (Consume)
            sqlx::query("DELETE FROM refresh_tokens WHERE token_hash = $1")
                .bind(token_hash)
                .execute(&mut *conn)
                .await?;

            Ok(Some(token.user_id))
        } else {
            tracing::warn!("Refresh token not found or already consumed (potential reuse attack)");
            Ok(None)
        }
    }

    /// Revokes a specific refresh token owned by the user (Logout).
    #[tracing::instrument(level = "debug", skip(self, conn, token_hash))]
    pub async fn delete_owned(&self, conn: &mut PgConnection, token_hash: &str, user_id: Uuid) -> Result<()> {
        sqlx::query("DELETE FROM refresh_tokens WHERE token_hash = $1 AND user_id = $2")
            .bind(token_hash)
            .bind(user_id)
            .execute(conn)
            .await
            .map_err(AppError::Database)?;
        Ok(())
    }
}
