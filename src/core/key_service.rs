use crate::config::MessagingConfig;
use crate::core::auth::verify_signature;
use crate::core::crypto_types::PublicKey;
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
    config: MessagingConfig,
}

pub struct KeyUploadParams {
    pub user_id: Uuid,
    pub identity_key: Option<PublicKey>,
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
        config: MessagingConfig,
    ) -> Self {
        Self { pool, key_repo, message_repo, notifier, config }
    }

    pub async fn get_pre_key_bundle(&self, user_id: Uuid) -> Result<Option<PreKeyBundle>> {
        let mut conn = self.pool.acquire().await?;
        self.key_repo.fetch_pre_key_bundle(&mut conn, user_id).await
    }

    pub async fn fetch_identity_key(&self, user_id: Uuid) -> Result<Option<PublicKey>> {
        self.key_repo.fetch_identity_key(&self.pool, user_id).await
    }

    pub async fn check_pre_key_status(&self, user_id: Uuid) -> Result<Option<PreKeyStatus>> {
        let count = self.key_repo.count_one_time_pre_keys(&self.pool, user_id).await?;
        if count < self.config.pre_key_refill_threshold as i64 {
            Ok(Some(PreKeyStatus {
                one_time_pre_key_count: count as i32,
                min_threshold: self.config.pre_key_refill_threshold,
            }))
        } else {
            Ok(None)
        }
    }

    /// Public entry point for key uploads.
    pub async fn upload_keys(&self, params: KeyUploadParams) -> Result<()> {
        let user_id = params.user_id;

        // If identity key is provided, we can verify the signature BEFORE starting a transaction.
        if let Some(ref ik) = params.identity_key {
            verify_keys(ik, &params.signed_pre_key)?;
        }

        let mut tx = self.pool.begin().await?;

        let is_takeover = self.upload_keys_internal(&mut tx, params).await?;

        tx.commit().await?;

        if is_takeover {
            self.notifier.notify(user_id, UserEvent::Disconnect);
        }

        Ok(())
    }

    /// Internal implementation that accepts a mutable connection.
    pub(crate) async fn upload_keys_internal(&self, conn: &mut PgConnection, params: KeyUploadParams) -> Result<bool> {
        let mut is_takeover = false;

        // 1. Identify/Verify Identity Key
        let ik = if let Some(new_ik) = params.identity_key {
            // Fetch existing identity key with LOCK
            let existing_ik_opt = self.key_repo.fetch_identity_key_for_update(&mut *conn, params.user_id).await?;

            if let Some(existing_ik) = existing_ik_opt {
                if existing_ik != new_ik {
                    is_takeover = true;
                }
            } else {
                is_takeover = true;
            }

            verify_keys(&new_ik, &params.signed_pre_key)?;
            new_ik
        } else {
            // Must exist
            let ik = self
                .key_repo
                .fetch_identity_key_for_update(&mut *conn, params.user_id)
                .await?
                .ok_or_else(|| AppError::BadRequest("Identity key missing".into()))?;

            // Verify signature with the stored key
            verify_keys(&ik, &params.signed_pre_key)?;
            ik
        };

        // 3. Limit Check (Atomic within transaction)
        let current_count =
            if is_takeover { 0 } else { self.key_repo.count_one_time_pre_keys(&mut *conn, params.user_id).await? };

        let new_keys_count = params.one_time_pre_keys.len() as i64;

        if current_count + new_keys_count > self.config.max_pre_keys {
            return Err(AppError::BadRequest(format!("Too many pre-keys. Limit is {}", self.config.max_pre_keys)));
        }

        // 4. Handle Takeover Cleanup
        if is_takeover {
            let reg_id = params
                .registration_id
                .ok_or_else(|| AppError::BadRequest("registrationId required for takeover".into()))?;

            self.key_repo.delete_all_signed_pre_keys(&mut *conn, params.user_id).await?;
            self.key_repo.delete_all_one_time_pre_keys(&mut *conn, params.user_id).await?;
            self.message_repo.delete_all_for_user(&mut *conn, params.user_id).await?;

            // Upsert Identity Key
            self.key_repo.upsert_identity_key(&mut *conn, params.user_id, &ik, reg_id).await?;
        }

        // 5. Common flow: Upsert Keys
        self.key_repo
            .upsert_signed_pre_key(
                &mut *conn,
                params.user_id,
                params.signed_pre_key.key_id,
                &params.signed_pre_key.public_key,
                &params.signed_pre_key.signature,
            )
            .await?;

        self.key_repo.insert_one_time_pre_keys(&mut *conn, params.user_id, &params.one_time_pre_keys).await?;

        Ok(is_takeover)
    }
}

fn verify_keys(ik: &PublicKey, signed_pre_key: &SignedPreKey) -> Result<()> {
    // Standard Signal Protocol behavior:
    // Verify signature over the 33-byte wire format of the SignedPreKey (0x05 prefix + 32-byte key)
    let spk_bytes = signed_pre_key.public_key.as_bytes();
    let signature = &signed_pre_key.signature;

    verify_signature(ik, spk_bytes, signature)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::core::crypto_types::{PublicKey, Signature};
    use curve25519_dalek::edwards::CompressedEdwardsY;
    use xeddsa::xed25519::PrivateKey;
    use xeddsa::{CalculateKeyPair, Sign};
    use rand::RngCore;
    use rand::rngs::OsRng;

    fn generate_keys() -> (PrivateKey, PublicKey, PrivateKey, PublicKey, Signature) {
        let mut ik_bytes = [0u8; 32];
        OsRng.fill_bytes(&mut ik_bytes);
        let ik = PrivateKey(ik_bytes);
        
        let (_, ik_pub_ed) = ik.calculate_key_pair(0);
        let ik_pub_mont = CompressedEdwardsY(ik_pub_ed)
            .decompress().unwrap().to_montgomery().to_bytes();
        let mut ik_pub_wire = [0u8; 33];
        ik_pub_wire[0] = 0x05;
        ik_pub_wire[1..].copy_from_slice(&ik_pub_mont);
        let ik_pub = PublicKey::new(ik_pub_wire);

        let mut spk_bytes = [0u8; 32];
        OsRng.fill_bytes(&mut spk_bytes);
        let spk = PrivateKey(spk_bytes);
        let (_, spk_pub_ed) = spk.calculate_key_pair(0);
        let spk_pub_mont = CompressedEdwardsY(spk_pub_ed)
            .decompress().unwrap().to_montgomery().to_bytes();
        let mut spk_pub_wire = [0u8; 33];
        spk_pub_wire[0] = 0x05;
        spk_pub_wire[1..].copy_from_slice(&spk_pub_mont);
        let spk_pub = PublicKey::new(spk_pub_wire);

        // Sign the WIRE format of SPK (33 bytes: prefix + raw X25519)
        let signature_bytes: [u8; 64] = ik.sign(spk_pub.as_bytes(), OsRng);
        let signature = Signature::new(signature_bytes);

        (ik, ik_pub, spk, spk_pub, signature)
    }

    #[test]
    fn test_verify_keys_client_format() {
        let (_, ik_pub, _, spk_pub, signature) = generate_keys();
        let spk = SignedPreKey { key_id: 1, public_key: spk_pub, signature };

        assert!(verify_keys(&ik_pub, &spk).is_ok());
    }
}