use crate::adapters::database::DbPool;
use crate::adapters::database::message_repo::MessageRepository;
use crate::adapters::database::user_repo::UserRepository;
use crate::domain::auth_session::AuthSession;
use crate::domain::crypto::PublicKey;
use crate::domain::keys::{OneTimePreKey, SignedPreKey};
use crate::domain::notification::UserEvent;
use crate::error::Result;
use crate::services::auth_service::AuthService;
use crate::services::key_service::{KeyService, KeyUploadParams};
use crate::services::notification_service::NotificationService;
use opentelemetry::{global, metrics::Counter};

#[derive(Clone, Debug)]
struct Metrics {
    registered_total: Counter<u64>,
    takeovers_total: Counter<u64>,
}

impl Metrics {
    fn new() -> Self {
        let meter = global::meter("obscura-server");
        Self {
            registered_total: meter
                .u64_counter("obscura_registrations_total")
                .with_description("Total number of successful user registrations")
                .build(),
            takeovers_total: meter
                .u64_counter("obscura_key_takeovers_total")
                .with_description("Total number of device takeover events")
                .build(),
        }
    }
}

#[derive(Clone, Debug)]
pub struct AccountService {
    pool: DbPool,
    user_repo: UserRepository,
    message_repo: MessageRepository,
    auth_service: AuthService,
    key_service: KeyService,
    notifier: NotificationService,
    metrics: Metrics,
}

impl AccountService {
    #[must_use]
    pub fn new(
        pool: DbPool,
        user_repo: UserRepository,
        message_repo: MessageRepository,
        auth_service: AuthService,
        key_service: KeyService,
        notifier: NotificationService,
    ) -> Self {
        Self { pool, user_repo, message_repo, auth_service, key_service, notifier, metrics: Metrics::new() }
    }

    /// Registers a new user account atomically.
    ///
    /// # Errors
    /// Returns `AppError::Conflict` if the username already exists.
    /// Returns `AppError::Database` if any of the underlying transactions fail.
    #[tracing::instrument(
        skip(self, username, password, identity_key, signed_pre_key, one_time_pre_keys),
        fields(user.id = tracing::field::Empty),
        err(level = "warn")
    )]
    pub(crate) async fn register(
        &self,
        username: String,
        password: String,
        identity_key: PublicKey,
        registration_id: i32,
        signed_pre_key: SignedPreKey,
        one_time_pre_keys: Vec<OneTimePreKey>,
    ) -> Result<AuthSession> {
        let password_hash = self.auth_service.hash_password(&password).await?;

        let mut tx = self.pool.begin().await?;

        // 1. Create User
        let user = self.user_repo.create(&mut tx, &username, &password_hash).await?;

        tracing::Span::current().record("user_id", tracing::field::display(user.id));

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
        self.metrics.registered_total.add(1, &[]);

        Ok(session)
    }

    /// Uploads new keys for an existing account.
    ///
    /// # Errors
    /// Returns `AppError::BadRequest` if key validation fails.
    /// Returns `AppError::Database` if the database operation fails.
    #[tracing::instrument(
        skip(self, params),
        fields(user.id = %params.user_id),
        err(level = "warn")
    )]
    pub(crate) async fn upload_keys(&self, params: KeyUploadParams) -> Result<()> {
        let user_id = params.user_id;

        let mut tx = self.pool.begin().await?;

        let is_takeover = self.key_service.upsert_keys(&mut tx, params).await?;

        if is_takeover {
            self.message_repo.delete_all_for_user(&mut tx, user_id).await?;
        }

        tx.commit().await?;

        if is_takeover {
            tracing::warn!("Device takeover detected");
            self.metrics.takeovers_total.add(1, &[]);

            self.notifier.notify(&[user_id], UserEvent::Disconnect).await;
        }

        Ok(())
    }
}
