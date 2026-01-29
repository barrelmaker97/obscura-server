use crate::config::AuthConfig;
use crate::core::auth::{self, create_jwt};
use crate::core::key_service::{KeyService, KeyUploadParams};
use crate::core::user::{OneTimePreKey, SignedPreKey};
use crate::error::{AppError, Result};
use crate::storage::DbPool;
use crate::storage::refresh_token_repo::RefreshTokenRepository;
use crate::storage::user_repo::UserRepository;
use serde::Serialize;
use uuid::Uuid;

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct AuthResponse {
    pub token: String,
    pub refresh_token: String,
    pub expires_at: i64,
}

#[derive(Clone)]
pub struct AccountService {
    pool: DbPool,
    config: AuthConfig,
    key_service: KeyService,
    user_repo: UserRepository,
    refresh_repo: RefreshTokenRepository,
}

impl AccountService {
    pub fn new(
        pool: DbPool,
        config: AuthConfig,
        key_service: KeyService,
        user_repo: UserRepository,
        refresh_repo: RefreshTokenRepository,
    ) -> Self {
        Self { pool, config, key_service, user_repo, refresh_repo }
    }

    pub async fn register(
        &self,
        username: String,
        password: String,
        identity_key: crate::core::crypto_types::PublicKey,
        registration_id: i32,
        signed_pre_key: SignedPreKey,
        one_time_pre_keys: Vec<OneTimePreKey>,
    ) -> Result<AuthResponse> {
        // 0. Uniqueness check (CPU only, outside transaction)
        KeyService::validate_otpk_uniqueness(&one_time_pre_keys)?;

        let password_hash: Result<String> =
            tokio::task::spawn_blocking(move || auth::hash_password(&password)).await.map_err(|e| {
                tracing::error!("Failed to spawn password hashing task: {}", e);
                AppError::Internal
            })?;
        let password_hash = password_hash.map_err(|e| {
            tracing::error!("Password hashing failed: {:?}", e);
            e
        })?;

        let mut tx = self.pool.begin().await?;

        let user = self.user_repo.create(&mut *tx, &username, &password_hash).await.map_err(|e| {
            if let AppError::Database(sqlx::Error::Database(db_err)) = &e
                && db_err.code().as_deref() == Some("23505")
            {
                return AppError::Conflict("Username already exists".into());
            }
            e
        })?;

        let key_params = KeyUploadParams {
            user_id: user.id,
            identity_key: Some(identity_key),
            registration_id: Some(registration_id),
            signed_pre_key,
            one_time_pre_keys,
        };

        self.key_service.upload_keys_internal(&mut tx, key_params).await?;

        // Generate Tokens
        let token = create_jwt(user.id, &self.config.jwt_secret, self.config.access_token_ttl_secs)?;
        let refresh_token = auth::generate_opaque_token();
        let refresh_hash = auth::hash_token(&refresh_token);

        self.refresh_repo.create(&mut *tx, user.id, &refresh_hash, self.config.refresh_token_ttl_days).await?;

        tx.commit().await?;

        tracing::info!("User registered successfully: {}", user.id);

        let expires_at = (time::OffsetDateTime::now_utc()
            + time::Duration::seconds(self.config.access_token_ttl_secs as i64))
        .unix_timestamp();

        Ok(AuthResponse { token, refresh_token, expires_at })
    }

    pub async fn login(&self, username: String, password: String) -> Result<AuthResponse> {
        let user = match self.user_repo.find_by_username(&self.pool, &username).await? {
            Some(u) => u,
            None => {
                tracing::info!("Login failed: User not found");
                return Err(AppError::AuthError);
            }
        };

        let password_hash = user.password_hash.clone();

        let is_valid: Result<bool> =
            tokio::task::spawn_blocking(move || auth::verify_password(&password, &password_hash)).await.map_err(
                |e| {
                    tracing::error!("Failed to spawn password verification task: {}", e);
                    AppError::Internal
                },
            )?;
        let is_valid = is_valid.map_err(|e| {
            tracing::error!("Password verification failed: {:?}", e);
            e
        })?;

        if !is_valid {
            tracing::info!("Login failed: Invalid password");
            return Err(AppError::AuthError);
        }

        // Generate Tokens
        let token = create_jwt(user.id, &self.config.jwt_secret, self.config.access_token_ttl_secs)?;
        let refresh_token = auth::generate_opaque_token();
        let refresh_hash = auth::hash_token(&refresh_token);

        let mut tx = self.pool.begin().await?;
        self.refresh_repo.create(&mut *tx, user.id, &refresh_hash, self.config.refresh_token_ttl_days).await?;
        tx.commit().await?;

        tracing::info!("User logged in successfully: {}", user.id);

        let expires_at = (time::OffsetDateTime::now_utc()
            + time::Duration::seconds(self.config.access_token_ttl_secs as i64))
        .unix_timestamp();

        Ok(AuthResponse { token, refresh_token, expires_at })
    }

    pub async fn refresh(&self, refresh_token: String) -> Result<AuthResponse> {
        // 1. Hash the incoming token to look it up
        let hash = auth::hash_token(&refresh_token);

        // 2. Verify and Rotate (Atomic Transaction)
        let mut tx = self.pool.begin().await?;

        let user_id = self.refresh_repo.verify_and_consume(&mut tx, &hash).await?.ok_or(AppError::AuthError)?;

        // 3. Generate New Pair
        let new_access_token = create_jwt(user_id, &self.config.jwt_secret, self.config.access_token_ttl_secs)?;
        let new_refresh_token = auth::generate_opaque_token();
        let new_refresh_hash = auth::hash_token(&new_refresh_token);

        // 4. Store New Refresh Token
        self.refresh_repo.create(&mut *tx, user_id, &new_refresh_hash, self.config.refresh_token_ttl_days).await?;

        tx.commit().await?;

        let expires_at = (time::OffsetDateTime::now_utc()
            + time::Duration::seconds(self.config.access_token_ttl_secs as i64))
        .unix_timestamp();

        Ok(AuthResponse { token: new_access_token, refresh_token: new_refresh_token, expires_at })
    }

    pub async fn logout(&self, user_id: Uuid, refresh_token: String) -> Result<()> {
        let hash = auth::hash_token(&refresh_token);

        self.refresh_repo.delete_owned(&self.pool, &hash, user_id).await?;

        tracing::info!("User logged out: {}", user_id);

        Ok(())
    }
}
