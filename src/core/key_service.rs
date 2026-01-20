use crate::config::Config;
use crate::core::notification::{Notifier, UserEvent};
use crate::core::user::{OneTimePreKey, PreKeyBundle, SignedPreKey};
use crate::error::{AppError, Result};
use crate::proto::obscura::v1::PreKeyStatus;
use crate::storage::key_repo::KeyRepository;
use crate::storage::message_repo::MessageRepository;
use sqlx::{PgConnection, PgPool};
use std::sync::Arc;
use uuid::Uuid;

#[derive(Clone)]
pub struct KeyService {
    pool: PgPool,
    key_repo: KeyRepository,
    message_repo: MessageRepository,
    notifier: Arc<dyn Notifier>,
    config: Config,
}

pub struct KeyUploadParams {
    pub user_id: Uuid,
    pub identity_key: Option<Vec<u8>>,
    pub registration_id: Option<i32>,
    pub signed_pre_key: SignedPreKey,
    pub one_time_pre_keys: Vec<OneTimePreKey>,
}

impl KeyService {
    pub fn new(
        pool: PgPool,
        key_repo: KeyRepository,
        message_repo: MessageRepository,
        notifier: Arc<dyn Notifier>,
        config: Config,
    ) -> Self {
        Self { pool, key_repo, message_repo, notifier, config }
    }

    pub async fn get_pre_key_bundle(&self, user_id: Uuid) -> Result<Option<PreKeyBundle>> {
        self.key_repo.fetch_pre_key_bundle(user_id).await
    }

    pub async fn fetch_identity_key(&self, user_id: Uuid) -> Result<Option<Vec<u8>>> {
        self.key_repo.fetch_identity_key(user_id).await
    }

    pub async fn check_pre_key_status(&self, user_id: Uuid) -> Result<Option<PreKeyStatus>> {
        let count = self.key_repo.count_one_time_pre_keys(&self.pool, user_id).await?;
        if count < self.config.messaging.pre_key_refill_threshold as i64 {
            Ok(Some(PreKeyStatus {
                one_time_pre_key_count: count as i32,
                min_threshold: self.config.messaging.pre_key_refill_threshold,
            }))
        } else {
            Ok(None)
        }
    }

    /// Public entry point for key uploads.
    pub async fn upload_keys(&self, params: KeyUploadParams) -> Result<()> {
        let mut tx = self.pool.begin().await?;
        let user_id = params.user_id;

        let is_takeover = self.upload_keys_internal(&mut *tx, params).await?;

        tx.commit().await?;

        if is_takeover {
            self.notifier.notify(user_id, UserEvent::Disconnect);
        }

        Ok(())
    }

    /// Internal implementation that accepts a mutable connection.
    pub async fn upload_keys_internal(
        &self,
        conn: &mut PgConnection,
        params: KeyUploadParams,
    ) -> Result<bool> {
        let mut is_takeover = false;

        // 1. Check Identity Key if provided
        if let Some(new_ik_bytes) = &params.identity_key {
            // Fetch existing identity key with LOCK
            let existing_ik_opt = self.key_repo.fetch_identity_key_for_update(&mut *conn, params.user_id).await?;

            if let Some(existing_ik) = existing_ik_opt {
                if existing_ik != *new_ik_bytes {
                    is_takeover = true;
                }
            } else {
                // No existing key? Treat as takeover to ensure clean slate.
                is_takeover = true;
            }
        }

        // 2. Limit Check (Atomic within transaction)
        let current_count = if is_takeover {
            0
        } else {
            self.key_repo.count_one_time_pre_keys(&mut *conn, params.user_id).await?
        };

        let new_keys_count = params.one_time_pre_keys.len() as i64;

        if current_count + new_keys_count > self.config.messaging.max_pre_keys {
            return Err(AppError::BadRequest(format!(
                "Too many pre-keys. Limit is {}",
                self.config.messaging.max_pre_keys
            )));
        }

        // 3. Handle Takeover Cleanup
        if is_takeover {
            let reg_id = params
                .registration_id
                .ok_or_else(|| AppError::BadRequest("registrationId required for takeover".into()))?;

            self.key_repo.delete_all_signed_pre_keys(&mut *conn, params.user_id).await?;
            self.key_repo.delete_all_one_time_pre_keys(&mut *conn, params.user_id).await?;
            self.message_repo.delete_all_for_user(&mut *conn, params.user_id).await?;

            if let Some(ik) = &params.identity_key {
                self.key_repo.upsert_identity_key(&mut *conn, params.user_id, ik, reg_id).await?;
            }
        }

        // 4. Common flow: Upsert Keys
        self.key_repo
            .upsert_signed_pre_key(
                &mut *conn,
                params.user_id,
                params.signed_pre_key.key_id,
                &params.signed_pre_key.public_key,
                &params.signed_pre_key.signature,
            )
            .await?;

        let otpk_vec: Vec<(i32, Vec<u8>)> =
            params.one_time_pre_keys.into_iter().map(|k| (k.key_id, k.public_key)).collect();

        self.key_repo.insert_one_time_pre_keys(&mut *conn, params.user_id, &otpk_vec).await?;

        Ok(is_takeover)
    }
}