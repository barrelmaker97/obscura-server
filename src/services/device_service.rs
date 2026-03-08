use crate::adapters::database::DbPool;
use crate::adapters::database::device_repo::DeviceRepository;
use crate::adapters::database::message_repo::MessageRepository;
use crate::domain::auth_session::AuthSession;
use crate::domain::crypto::PublicKey;
use crate::domain::device::Device;
use crate::domain::keys::{OneTimePreKey, SignedPreKey};
use crate::domain::notification::UserEvent;
use crate::error::{AppError, Result};
use crate::services::auth_service::AuthService;
use crate::services::key_service::{KeyService, KeyUploadParams};
use crate::services::notification_service::NotificationService;
use opentelemetry::{global, metrics::Counter};
use uuid::Uuid;

#[derive(Clone, Debug)]
struct Metrics {
    devices_created: Counter<u64>,
    devices_deleted: Counter<u64>,
    takeovers: Counter<u64>,
}

impl Metrics {
    fn new() -> Self {
        let meter = global::meter("obscura-server");
        Self {
            devices_created: meter
                .u64_counter("obscura_devices_created_total")
                .with_description("Total number of devices provisioned")
                .build(),
            devices_deleted: meter
                .u64_counter("obscura_devices_deleted_total")
                .with_description("Total number of devices deleted")
                .build(),
            takeovers: meter
                .u64_counter("obscura_key_takeovers_total")
                .with_description("Total number of device takeover events")
                .build(),
        }
    }
}

#[derive(Clone, Debug)]
pub struct DeviceService {
    pool: DbPool,
    device_repo: DeviceRepository,
    message_repo: MessageRepository,
    key_service: KeyService,
    auth_service: AuthService,
    notifier: NotificationService,
    metrics: Metrics,
    max_devices_per_user: i64,
}

impl DeviceService {
    #[must_use]
    pub fn new(
        pool: DbPool,
        device_repo: DeviceRepository,
        message_repo: MessageRepository,
        key_service: KeyService,
        auth_service: AuthService,
        notifier: NotificationService,
        max_devices_per_user: i64,
    ) -> Self {
        Self {
            pool,
            device_repo,
            message_repo,
            key_service,
            auth_service,
            notifier,
            metrics: Metrics::new(),
            max_devices_per_user,
        }
    }

    /// Creates a new device, uploads its keys, and returns a full JWT.
    ///
    /// # Errors
    /// Returns `AppError::Database` if any database operation fails.
    #[tracing::instrument(
        skip(self, identity_key, signed_pre_key, one_time_pre_keys),
        fields(user.id = %user_id, device.id = tracing::field::Empty),
        err(level = "warn")
    )]
    pub(crate) async fn create_device(
        &self,
        user_id: Uuid,
        name: Option<String>,
        identity_key: PublicKey,
        registration_id: i32,
        signed_pre_key: SignedPreKey,
        one_time_pre_keys: Vec<OneTimePreKey>,
    ) -> Result<AuthSession> {
        let mut conn = self.pool.acquire().await?;
        let current_device_count = self.device_repo.count_by_user(&mut conn, user_id).await?;
        if current_device_count >= self.max_devices_per_user {
            return Err(AppError::Forbidden(format!(
                "Device limit reached. Maximum allowed is {}.",
                self.max_devices_per_user
            )));
        }
        drop(conn);

        let mut tx = self.pool.begin().await?;

        // 1. Create Device
        let device = self.device_repo.create(&mut tx, user_id, name.as_deref()).await?;

        tracing::Span::current().record("device.id", tracing::field::display(device.id));

        // 2. Upload Keys
        let key_params = KeyUploadParams {
            device_id: device.id,
            identity_key: Some(identity_key),
            registration_id: Some(registration_id),
            signed_pre_key,
            one_time_pre_keys,
        };

        self.key_service.upsert_keys(&mut tx, key_params).await?;

        // 3. Create Session with full JWT (includes device_id)
        let session = self.auth_service.create_session(&mut tx, user_id, Some(device.id)).await?;

        tx.commit().await?;

        tracing::info!("Device provisioned successfully");
        self.metrics.devices_created.add(1, &[]);

        Ok(session)
    }

    /// Uploads new keys for an existing device. Handles takeover if identity key changes.
    ///
    /// # Errors
    /// Returns `AppError::BadRequest` if key validation fails.
    /// Returns `AppError::Database` if the database operation fails.
    #[tracing::instrument(
        skip(self, params),
        fields(device.id = %params.device_id),
        err(level = "warn")
    )]
    pub(crate) async fn upload_keys(&self, params: KeyUploadParams) -> Result<()> {
        let device_id = params.device_id;

        let mut tx = self.pool.begin().await?;

        let is_takeover = self.key_service.upsert_keys(&mut tx, params).await?;

        if is_takeover {
            self.message_repo.delete_all_for_device(&mut tx, device_id).await?;
        }

        tx.commit().await?;

        if is_takeover {
            tracing::warn!("Device takeover detected");
            self.metrics.takeovers.add(1, &[]);

            self.notifier.notify(&[device_id], UserEvent::Disconnect).await;
        }

        Ok(())
    }

    /// Lists all devices for a user.
    ///
    /// # Errors
    /// Returns `AppError::Database` if the query fails.
    #[tracing::instrument(skip(self), fields(user.id = %user_id), err)]
    pub(crate) async fn list_devices(&self, user_id: Uuid) -> Result<Vec<Device>> {
        let mut conn = self.pool.acquire().await?;
        self.device_repo.find_by_user(&mut conn, user_id).await
    }

    /// Deletes a device owned by the user. Cascade delete handles keys/messages/push tokens.
    ///
    /// # Errors
    /// Returns `AppError::NotFound` if the device doesn't exist or isn't owned by the user.
    #[tracing::instrument(skip(self), fields(user.id = %user_id, device.id = %device_id), err)]
    pub(crate) async fn delete_device(&self, device_id: Uuid, user_id: Uuid) -> Result<()> {
        let mut conn = self.pool.acquire().await?;
        let deleted = self.device_repo.delete(&mut conn, device_id, user_id).await?;

        if !deleted {
            return Err(AppError::NotFound);
        }

        tracing::info!("Device deleted");
        self.metrics.devices_deleted.add(1, &[]);

        Ok(())
    }
}
