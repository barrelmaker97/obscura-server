use crate::config::AuthConfig;
use crate::core::auth::{self, create_jwt, AuthResponse};
use crate::error::{AppError, Result};
use crate::storage::refresh_token_repo::RefreshTokenRepository;
use crate::storage::DbPool;
use sqlx::{Executor, Postgres};
use uuid::Uuid;

#[derive(Clone)]
pub struct AuthService {
    config: AuthConfig,
    refresh_repo: RefreshTokenRepository,
}

impl AuthService {
    pub fn new(config: AuthConfig, refresh_repo: RefreshTokenRepository) -> Self {
        Self { config, refresh_repo }
    }

    pub async fn hash_password(&self, password: &str) -> Result<String> {
        let password = password.to_string();
        tokio::task::spawn_blocking(move || auth::hash_password(&password))
            .await
            .map_err(|_| AppError::Internal)?
    }

    pub async fn verify_password(&self, password: &str, password_hash: &str) -> Result<bool> {
        let password = password.to_string();
        let password_hash = password_hash.to_string();
        tokio::task::spawn_blocking(move || auth::verify_password(&password, &password_hash))
            .await
            .map_err(|_| AppError::Internal)?
    }

    pub async fn create_session<'e, E>(&self, executor: E, user_id: Uuid) -> Result<AuthResponse>
    where
        E: Executor<'e, Database = Postgres>,
    {
        let token = create_jwt(user_id, &self.config.jwt_secret, self.config.access_token_ttl_secs)?;
        let refresh_token = auth::generate_opaque_token();
        let refresh_hash = auth::hash_token(&refresh_token);

        self.refresh_repo.create(executor, user_id, &refresh_hash, self.config.refresh_token_ttl_days).await?;

        let expires_at = (time::OffsetDateTime::now_utc()
            + time::Duration::seconds(self.config.access_token_ttl_secs as i64))
        .unix_timestamp();

        Ok(AuthResponse { token, refresh_token, expires_at })
    }

    pub async fn refresh_session(&self, pool: &DbPool, refresh_token: String) -> Result<AuthResponse> {
        // 1. Hash the incoming token to look it up
        let hash = auth::hash_token(&refresh_token);

        // 2. Verify and Rotate (Atomic Transaction)
        let mut tx = pool.begin().await?;

        let user_id = self.refresh_repo.verify_and_consume(&mut tx, &hash).await?.ok_or(AppError::AuthError)?;

        tracing::Span::current().record("user.id", tracing::field::display(user_id));

        // 3. Generate New Pair
        let new_access_token = create_jwt(user_id, &self.config.jwt_secret, self.config.access_token_ttl_secs)?;
        let new_refresh_token = auth::generate_opaque_token();
        let new_refresh_hash = auth::hash_token(&new_refresh_token);

        // 4. Store New Refresh Token
        self.refresh_repo.create(&mut *tx, user_id, &new_refresh_hash, self.config.refresh_token_ttl_days).await?;

        tx.commit().await?;

        tracing::info!("Tokens rotated successfully");

        let expires_at = (time::OffsetDateTime::now_utc()
            + time::Duration::seconds(self.config.access_token_ttl_secs as i64))
        .unix_timestamp();

        Ok(AuthResponse { token: new_access_token, refresh_token: new_refresh_token, expires_at })
    }

    pub async fn logout(&self, pool: &DbPool, user_id: Uuid, refresh_token: String) -> Result<()> {
        let hash = auth::hash_token(&refresh_token);
        self.refresh_repo.delete_owned(pool, &hash, user_id).await?;
        Ok(())
    }
}
