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
}
