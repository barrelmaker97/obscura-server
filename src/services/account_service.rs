use crate::services::auth_service::AuthService;
use crate::services::key_service::{KeyService, KeyUploadParams};
use crate::services::message_service::MessageService;
use crate::services::notification_service::{NotificationService, UserEvent};
use crate::domain::keys::{OneTimePreKey, SignedPreKey};
use crate::domain::auth_session::AuthSession;
use crate::error::{AppError, Result};
use crate::storage::DbPool;
use crate::storage::user_repo::UserRepository;
use opentelemetry::{global, metrics::Counter};
use std::sync::Arc;
use uuid::Uuid;

#[derive(Clone)]
struct AccountMetrics {
    users_registered_total: Counter<u64>,
    keys_takeovers_total: Counter<u64>,
}

impl AccountMetrics {
    fn new() -> Self {
        let meter = global::meter("obscura-server");
        Self {
            users_registered_total: meter
                .u64_counter("users_registered_total")
                .with_description("Total number of successful user registrations")
                .build(),
            keys_takeovers_total: meter
                .u64_counter("keys_takeovers_total")
                .with_description("Total number of device takeover events")
                .build(),
        }
    }
}

#[derive(Clone)]
pub struct AccountService {
    pool: DbPool,
    user_repo: UserRepository,
    auth_service: AuthService,
    key_service: KeyService,
    message_service: MessageService,
    notifier: Arc<dyn NotificationService>,
    metrics: AccountMetrics,
}

impl AccountService {
    pub fn new(
        pool: DbPool,
        user_repo: UserRepository,
        auth_service: AuthService,
        key_service: KeyService,
        message_service: MessageService,
        notifier: Arc<dyn NotificationService>,
    ) -> Self {
        Self {
            pool,
            user_repo,
            auth_service,
            key_service,
            message_service,
            notifier,
            metrics: AccountMetrics::new(),
        }
    }

    #[tracing::instrument(
        skip(self, username, password, identity_key, signed_pre_key, one_time_pre_keys),
        fields(user_id = tracing::field::Empty),
        err(level = "warn")
    )]
    pub async fn register(
        &self,
        username: String,
        password: String,
        identity_key: crate::domain::crypto::PublicKey,
        registration_id: i32,
        signed_pre_key: SignedPreKey,
        one_time_pre_keys: Vec<OneTimePreKey>,
    ) -> Result<AuthSession> {
        let password_hash = self.auth_service.hash_password(&password).await?;

        let mut tx = self.pool.begin().await?;

        // 1. Create User
        let user = self.user_repo.create(&mut tx, &username, &password_hash).await?;

        tracing::Span::current().record("user.id", tracing::field::display(user.id));

        // 2. Upload Keys
        let key_params = KeyUploadParams {
            user_id: user.id,
            identity_key: Some(identity_key),
            registration_id: Some(registration_id),
            signed_pre_key,
            one_time_pre_keys,
        };

        self.key_service.upsert_keys(&mut tx, key_params).await?;

        // 3. Create Session (Auth)
        let session = self.auth_service.create_session(&mut tx, user.id).await?;

        tx.commit().await?;

        tracing::info!("User registered successfully");
        self.metrics.users_registered_total.add(1, &[]);

        Ok(session)
    }

    #[tracing::instrument(
        skip(self, params),
        fields(user_id = %params.user_id),
        err(level = "warn")
    )]
    pub async fn upload_keys(&self, params: KeyUploadParams) -> Result<()> {
        let user_id = params.user_id;

        let mut tx = self.pool.begin().await?;

        let is_takeover = self.key_service.upsert_keys(&mut tx, params).await?;

        if is_takeover {
            self.message_service.delete_all_for_user(&mut tx, user_id).await?;
        }

        tx.commit().await?;

        if is_takeover {
            tracing::warn!("Device takeover detected");
            self.metrics.keys_takeovers_total.add(1, &[]);

            self.notifier.notify(user_id, UserEvent::Disconnect);
        }

        Ok(())
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

        let is_valid = self.auth_service.verify_password(&password, &user.password_hash).await?;

        if !is_valid {
            tracing::Span::current().record("user_id", tracing::field::display(user.id));
            tracing::warn!("Login failed: invalid password");
            return Err(AppError::AuthError);
        }

        // Generate Tokens
        let mut tx = self.pool.begin().await?;
        let session = self.auth_service.create_session(&mut tx, user.id).await?;
        tx.commit().await?;

        tracing::info!("User logged in successfully");

        Ok(session)
    }

    #[tracing::instrument(
        skip(self, refresh_token),
        fields(user_id = tracing::field::Empty),
        err(level = "warn")
    )]
    pub async fn refresh(&self, refresh_token: String) -> Result<AuthSession> {
        self.auth_service.refresh_session(&self.pool, refresh_token).await
    }

    #[tracing::instrument(err, skip(self, refresh_token), fields(user_id = %user_id))]
    pub async fn logout(&self, user_id: Uuid, refresh_token: String) -> Result<()> {
        let mut conn = self.pool.acquire().await?;
        self.auth_service.logout(&mut conn, user_id, refresh_token).await?;
        tracing::info!("User logged out");
        Ok(())
    }
}
