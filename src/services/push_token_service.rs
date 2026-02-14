use crate::adapters::database::push_token_repo::PushTokenRepository;
use crate::adapters::database::DbPool;
use crate::error::Result;
use uuid::Uuid;

#[derive(Clone, Debug)]
pub struct PushTokenService {
    pool: DbPool,
    repo: PushTokenRepository,
}

impl PushTokenService {
    pub fn new(pool: DbPool, repo: PushTokenRepository) -> Self {
        Self { pool, repo }
    }

    /// Registers or updates a push token for a user.
    pub async fn register_token(&self, user_id: Uuid, token: String) -> Result<()> {
        // Here we could add validation (e.g. token format checks)
        let mut conn = self.pool.acquire().await?;
        self.repo.upsert_token(&mut conn, user_id, &token).await
    }

    /// Retrieves all tokens for a batch of users.
    pub async fn get_tokens_for_users(&self, user_ids: &[Uuid]) -> Result<Vec<(Uuid, String)>> {
        let mut conn = self.pool.acquire().await?;
        self.repo.find_tokens_for_users(&mut conn, user_ids).await
    }

    /// Invalidate a token (e.g. when the provider reports it as unregistered).
    pub async fn invalidate_token(&self, token: &str) -> Result<()> {
        let mut conn = self.pool.acquire().await?;
        self.repo.delete_token(&mut conn, token).await
    }
}
