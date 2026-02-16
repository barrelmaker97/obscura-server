use crate::error::Result;
use sqlx::PgConnection;
use uuid::Uuid;

#[derive(Clone, Debug, Default)]
pub struct PushTokenRepository {}

impl PushTokenRepository {
    #[must_use]
    pub const fn new() -> Self {
        Self {}
    }

    /// Register or update a push token for a user.
    ///
    /// # Errors
    /// Returns a database error if the upsert fails.
    #[tracing::instrument(level = "debug", skip(self, conn, token), err)]
    pub async fn upsert_token(&self, conn: &mut PgConnection, user_id: Uuid, token: &str) -> Result<()> {
        sqlx::query(
            r#"
            INSERT INTO push_tokens (user_id, token, updated_at)
            VALUES ($1, $2, NOW())
            ON CONFLICT (user_id) DO UPDATE
            SET token = $2, updated_at = NOW()
            "#,
        )
        .bind(user_id)
        .bind(token)
        .execute(conn)
        .await?;
        Ok(())
    }

    /// Finds tokens for a batch of users.
    /// Returns a list of (`user_id`, token) pairs.
    ///
    /// # Errors
    /// Returns a database error if the query fails.
    #[tracing::instrument(level = "debug", skip(self, conn), err)]
    pub(crate) async fn find_tokens_for_users(
        &self,
        conn: &mut PgConnection,
        user_ids: &[Uuid],
    ) -> Result<Vec<(Uuid, String)>> {
        let rows =
            sqlx::query_as::<_, (Uuid, String)>("SELECT user_id, token FROM push_tokens WHERE user_id = ANY($1)")
                .bind(user_ids)
                .fetch_all(conn)
                .await?;

        Ok(rows)
    }

    /// Deletes a batch of push tokens.
    ///
    /// # Errors
    /// Returns a database error if the deletion fails.
    #[tracing::instrument(level = "debug", skip(self, conn), err)]
    pub async fn delete_tokens_batch(&self, conn: &mut PgConnection, tokens: &[String]) -> Result<()> {
        if tokens.is_empty() {
            return Ok(());
        }
        sqlx::query("DELETE FROM push_tokens WHERE token = ANY($1)").bind(tokens).execute(conn).await?;
        Ok(())
    }
}
