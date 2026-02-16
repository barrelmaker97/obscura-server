use crate::adapters::database::DbPool;
use crate::adapters::database::push_token_repo::PushTokenRepository;
use crate::error::Result;
use uuid::Uuid;

#[derive(Clone, Debug)]
pub struct PushTokenService {
    pool: DbPool,
    repo: PushTokenRepository,
}

impl PushTokenService {
    #[must_use]
    pub const fn new(pool: DbPool, repo: PushTokenRepository) -> Self {
        Self { pool, repo }
    }

    /// Registers or updates a push token for a user.
    ///
    /// # Errors
    /// Returns an error if the database operation fails.
    pub async fn register_token(&self, user_id: Uuid, token: String) -> Result<()> {
        // Here we could add validation (e.g. token format checks)
        let mut conn = self.pool.acquire().await?;
        self.repo.upsert_token(&mut conn, user_id, &token).await
    }

    /// Retrieves all tokens for a batch of users.
    ///
    /// # Errors
    /// Returns an error if the database operation fails.
    pub async fn get_tokens_for_users(&self, user_ids: &[Uuid]) -> Result<Vec<(Uuid, String)>> {
        let mut conn = self.pool.acquire().await?;
        self.repo.find_tokens_for_users(&mut conn, user_ids).await
    }

    /// Invalidate a batch of tokens.
    ///
    /// # Errors
    /// Returns an error if the database operation fails.
    pub async fn invalidate_tokens_batch(&self, tokens: &[String]) -> Result<()> {
        if tokens.is_empty() {
            return Ok(());
        }
        let mut conn = self.pool.acquire().await?;
        self.repo.delete_tokens_batch(&mut conn, tokens).await
    }
}
