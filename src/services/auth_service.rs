use crate::config::AuthConfig;
use crate::domain::auth::{Claims, Jwt};
use crate::domain::auth_session::AuthSession;
use crate::error::{AppError, Result};
use crate::storage::refresh_token_repo::RefreshTokenRepository;
use crate::storage::user_repo::UserRepository;
use argon2::{
    Argon2,
    password_hash::{PasswordHash, PasswordHasher, PasswordVerifier, SaltString},
};
use base64::Engine;
use jsonwebtoken::{DecodingKey, EncodingKey, Header, Validation, decode, encode};
use opentelemetry::{global, metrics::Counter};
use rand::{RngCore, rngs::OsRng};
use sha2::{Digest, Sha256};
use sqlx::PgConnection;
use std::time::{SystemTime, UNIX_EPOCH};
use uuid::Uuid;

#[derive(Clone)]
struct Metrics {
    login_total: Counter<u64>,
    refresh_total: Counter<u64>,
    logout_total: Counter<u64>,
}

impl Metrics {
    fn new() -> Self {
        let meter = global::meter("obscura-server");
        Self {
            login_total: meter
                .u64_counter("auth_login_total")
                .with_description("Total number of successful login attempts")
                .build(),
            refresh_total: meter
                .u64_counter("auth_refresh_total")
                .with_description("Total number of successful token rotations")
                .build(),
            logout_total: meter
                .u64_counter("auth_logout_total")
                .with_description("Total number of successful logout attempts")
                .build(),
        }
    }
}

#[derive(Clone)]
pub struct AuthService {
    config: AuthConfig,
    pool: crate::storage::DbPool,
    user_repo: UserRepository,
    refresh_repo: RefreshTokenRepository,
    metrics: Metrics,
}

impl AuthService {
    pub fn new(config: AuthConfig, pool: crate::storage::DbPool, user_repo: UserRepository, refresh_repo: RefreshTokenRepository) -> Self {
        Self {
            config,
            pool,
            user_repo,
            refresh_repo,
            metrics: Metrics::new(),
        }
    }

    #[tracing::instrument(
        skip(self, username, password),
        fields(user_id = tracing::field::Empty),
        err(level = "warn")
    )]
    pub async fn login(&self, username: String, password: String) -> Result<AuthSession> {
        let mut conn = self.pool.acquire().await?;
        let user = match self.user_repo.find_by_username(&mut conn, &username).await? {
            Some(u) => u,
            None => {
                tracing::warn!("Login failed: user not found");
                return Err(AppError::AuthError);
            }
        };

        tracing::Span::current().record("user.id", tracing::field::display(user.id));

        let is_valid = self.verify_password(&password, &user.password_hash).await?;

        if !is_valid {
            tracing::warn!("Login failed: invalid password");
            return Err(AppError::AuthError);
        }

        // Generate Tokens
        let session = self.create_session(&mut conn, user.id).await?;
        self.metrics.login_total.add(1, &[]);
        Ok(session)
    }

    #[tracing::instrument(err, skip(self, password))]
    pub async fn hash_password(&self, password: &str) -> Result<String> {
        let password = password.to_string();
        tokio::task::spawn_blocking(move || {
            let salt = SaltString::generate(&mut OsRng);
            let argon2 = Argon2::default();
            argon2
                .hash_password(password.as_bytes(), &salt)
                .map_err(|_| AppError::Internal)
                .map(|h| h.to_string())
        })
        .await
        .map_err(|_| AppError::Internal)?
    }

    #[tracing::instrument(err, skip(self, password, password_hash))]
    pub async fn verify_password(&self, password: &str, password_hash: &str) -> Result<bool> {
        let password = password.to_string();
        let password_hash = password_hash.to_string();
        tokio::task::spawn_blocking(move || {
            let parsed_hash = PasswordHash::new(&password_hash).map_err(|_| AppError::Internal)?;
            Ok(Argon2::default().verify_password(password.as_bytes(), &parsed_hash).is_ok())
        })
        .await
        .map_err(|_| AppError::Internal)?
    }

    #[tracing::instrument(err, skip(self, conn), fields(user_id = %user_id))]
    pub async fn create_session(&self, conn: &mut PgConnection, user_id: Uuid) -> Result<AuthSession> {
        let exp = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap_or(std::time::Duration::from_secs(0))
            .as_secs() as usize
            + self.config.access_token_ttl_secs as usize;

        let claims = Claims::new(user_id, exp);
        let jwt = self.encode_jwt(&claims)?;
        
        let refresh_token = self.generate_opaque_token();
        let refresh_hash = self.hash_opaque_token(&refresh_token);

        self.refresh_repo.create(conn, user_id, &refresh_hash, self.config.refresh_token_ttl_days).await?;

        Ok(AuthSession { 
            token: jwt.as_str().to_string(), 
            refresh_token, 
            expires_at: exp as i64 
        })
    }

    #[tracing::instrument(err, skip(self, refresh_token))]
    pub async fn refresh_session(&self, refresh_token: String) -> Result<AuthSession> {
        let mut conn = self.pool.acquire().await?;
        let old_hash = self.hash_opaque_token(&refresh_token);
        let new_refresh_token = self.generate_opaque_token();
        let new_hash = self.hash_opaque_token(&new_refresh_token);

        let user_id = self
            .refresh_repo
            .rotate(&mut conn, &old_hash, &new_hash, self.config.refresh_token_ttl_days)
            .await?
            .ok_or(AppError::AuthError)?;

        tracing::Span::current().record("user.id", tracing::field::display(user_id));

        let exp = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap_or(std::time::Duration::from_secs(0))
            .as_secs() as usize
            + self.config.access_token_ttl_secs as usize;

        let claims = Claims::new(user_id, exp);
        let new_jwt = self.encode_jwt(&claims)?;

        tracing::info!("Tokens rotated successfully");
        self.metrics.refresh_total.add(1, &[]);

        Ok(AuthSession {
            token: new_jwt.as_str().to_string(),
            refresh_token: new_refresh_token,
            expires_at: exp as i64,
        })
    }

    #[tracing::instrument(err, skip(self, refresh_token), fields(user_id = %user_id))]
    pub async fn logout(&self, user_id: Uuid, refresh_token: String) -> Result<()> {
        let mut conn = self.pool.acquire().await?;
        let hash = self.hash_opaque_token(&refresh_token);
        self.refresh_repo.delete_owned(&mut conn, &hash, user_id).await?;
        self.metrics.logout_total.add(1, &[]);
        Ok(())
    }
    /// Verifies a JWT access token and returns the user ID (subject).
    pub fn verify_token(&self, jwt: Jwt) -> Result<Uuid> {
        let token_data = decode::<Claims>(
            jwt.as_str(),
            &DecodingKey::from_secret(self.config.jwt_secret.as_bytes()),
            &Validation::default(),
        )
        .map_err(|_| AppError::AuthError)?;

        Ok(token_data.claims.sub)
    }

    fn encode_jwt(&self, claims: &Claims) -> Result<Jwt> {
        let token = encode(
            &Header::default(),
            claims,
            &EncodingKey::from_secret(self.config.jwt_secret.as_bytes()),
        )
        .map_err(|_| AppError::Internal)?;

        Ok(Jwt(token))
    }

    fn generate_opaque_token(&self) -> String {
        let mut bytes = [0u8; 32];
        OsRng.fill_bytes(&mut bytes);
        base64::engine::general_purpose::URL_SAFE_NO_PAD.encode(bytes)
    }

    fn hash_opaque_token(&self, token: &str) -> String {
        let mut hasher = Sha256::new();
        hasher.update(token.as_bytes());
        hex::encode(hasher.finalize())
    }
}
#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::AuthConfig;
    use crate::storage::refresh_token_repo::RefreshTokenRepository;

    fn setup_service() -> AuthService {
        let config = AuthConfig {
            jwt_secret: "test_secret".to_string(),
            access_token_ttl_secs: 3600,
            refresh_token_ttl_days: 7,
        };
        let pool = sqlx::PgPool::connect_lazy("postgres://localhost/test").unwrap();
        AuthService::new(config, pool, UserRepository::new(), RefreshTokenRepository::new())
    }

    #[tokio::test]
    async fn test_jwt_roundtrip() {
        let service = setup_service();
        let user_id = Uuid::new_v4();
        let exp = 10000000000;
        let claims = Claims::new(user_id, exp);

        let jwt = service.encode_jwt(&claims).unwrap();
        let decoded_id = service.verify_token(jwt).unwrap();

        assert_eq!(user_id, decoded_id);
    }

    #[tokio::test]
    async fn test_password_hashing() {
        let service = setup_service();
        let password = "password12345";
        let hash = service.hash_password(password).await.unwrap();

        assert!(service.verify_password(password, &hash).await.unwrap());
        assert!(!service.verify_password("wrong_password", &hash).await.unwrap());
    }

    #[tokio::test]
    async fn test_opaque_token_logic() {
        let service = setup_service();
        let token1 = service.generate_opaque_token();
        let token2 = service.generate_opaque_token();

        assert_ne!(token1, token2);

        let hash1 = service.hash_opaque_token(&token1);
        let hash2 = service.hash_opaque_token(&token1);
        assert_eq!(hash1, hash2);
    }
}
