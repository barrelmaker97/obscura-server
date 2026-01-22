use crate::config::Config;
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
    config: Config,
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
        config: Config,
    ) -> Self {
        Self { pool, key_repo, message_repo, notifier, config }
    }

    pub async fn get_pre_key_bundle(&self, user_id: Uuid) -> Result<Option<PreKeyBundle>> {
        let mut conn = self.pool.acquire().await?;
        self.key_repo.fetch_pre_key_bundle(&mut conn, user_id).await
    }

    pub async fn fetch_identity_key(&self, user_id: Uuid) -> Result<Option<Vec<u8>>> {
        self.key_repo.fetch_identity_key(&self.pool, user_id).await
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
        let user_id = params.user_id;

        // If identity key is provided, we can verify the signature BEFORE starting a transaction.
        // This avoids holding a DB lock during CPU-intensive crypto verification.
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
            let existing_ik_bytes_opt = self.key_repo.fetch_identity_key_for_update(&mut *conn, params.user_id).await?;

            if let Some(existing_ik_bytes) = existing_ik_bytes_opt {
                 // Compare bytes (new_ik.to_wire_bytes() vs existing DB bytes)
                 let new_ik_bytes = new_ik.to_wire_bytes();
                 if existing_ik_bytes != new_ik_bytes {
                    is_takeover = true;
                }
            } else {
                // No existing key? Treat as takeover to ensure clean slate.
                is_takeover = true;
            }

            // Note: Verification was likely already done in the public wrapper,
            // but we keep it here for safety (in case it's called internally).
            // This is fast if already verified because the math is the same.
            verify_keys(&new_ik, &params.signed_pre_key)?;
            new_ik
        } else {
             // Must exist
             let bytes = self.key_repo
                .fetch_identity_key_for_update(&mut *conn, params.user_id)
                .await?
                .ok_or_else(|| AppError::BadRequest("Identity key missing".into()))?;
             
             let ik = PublicKey::try_from(bytes).map_err(|_| AppError::Internal)?;
             
             // Verify signature with the stored key
             verify_keys(&ik, &params.signed_pre_key)?;
             ik
        };

        // 3. Limit Check (Atomic within transaction)
        let current_count =
            if is_takeover { 0 } else { self.key_repo.count_one_time_pre_keys(&mut *conn, params.user_id).await? };

        let new_keys_count = params.one_time_pre_keys.len() as i64;

        if current_count + new_keys_count > self.config.messaging.max_pre_keys {
            return Err(AppError::BadRequest(format!(
                "Too many pre-keys. Limit is {}",
                self.config.messaging.max_pre_keys
            )));
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
    let spk_bytes_full = signed_pre_key.public_key.to_wire_bytes();
    let spk_bytes_inner = signed_pre_key.public_key.clone().into_inner();
    let signature = signed_pre_key.signature.as_bytes();

    // Verification Attempt Helper
    let try_verify = |verifier_ik: &[u8], is_montgomery: bool| -> bool {
         // 1. Try full bytes
         if is_montgomery {
             if crate::core::auth::verify_signature_with_montgomery(verifier_ik, &spk_bytes_full, signature).is_ok() {
                 return true;
             }
         } else {
             if verify_signature(verifier_ik, &spk_bytes_full, signature).is_ok() {
                 return true;
             }
         }

         // 2. Try inner bytes (if different)
         if spk_bytes_full.len() != spk_bytes_inner.len() {
              if is_montgomery {
                if crate::core::auth::verify_signature_with_montgomery(verifier_ik, &spk_bytes_inner, signature).is_ok() {
                    return true;
                }
             } else {
                 if verify_signature(verifier_ik, &spk_bytes_inner, signature).is_ok() {
                     return true;
                 }
             }
         }
         false
    };

    match ik {
        PublicKey::Edwards(bytes) => {
             if try_verify(bytes, false) { return Ok(()); }
        },
        PublicKey::Montgomery(bytes) => {
             if try_verify(bytes, true) { return Ok(()); }
             if try_verify(bytes, false) { return Ok(()); }
        }
    }

    Err(AppError::BadRequest("Invalid signature".into()))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::core::crypto_types::{PublicKey, Signature};
    use ed25519_dalek::{Signer, SigningKey};
    use rand::RngCore;
    use rand::rngs::OsRng;

    fn generate_keys() -> (SigningKey, PublicKey, SigningKey, PublicKey, Signature) {
        let mut ik_bytes = [0u8; 32];
        OsRng.fill_bytes(&mut ik_bytes);
        let ik = SigningKey::from_bytes(&ik_bytes);
        let ik_pub_bytes = ik.verifying_key().to_bytes();
        let ik_pub = PublicKey::Edwards(ik_pub_bytes);

        let mut spk_bytes = [0u8; 32];
        OsRng.fill_bytes(&mut spk_bytes);
        let spk = SigningKey::from_bytes(&spk_bytes);
        let spk_pub_bytes = spk.verifying_key().to_bytes();
        let spk_pub = PublicKey::Edwards(spk_pub_bytes);

        // Sign the WIRE format of SPK (32 bytes here)
        let signature_bytes = ik.sign(&spk_pub.to_wire_bytes()).to_bytes();
        let signature = Signature::try_from(&signature_bytes[..]).unwrap();

        (ik, ik_pub, spk, spk_pub, signature)
    }

    #[test]
    fn test_verify_keys_standard_strict() {
        let (_, ik_pub, _, spk_pub, signature) = generate_keys();
        let spk = SignedPreKey { key_id: 1, public_key: spk_pub, signature };

        assert!(verify_keys(&ik_pub, &spk).is_ok());
    }
}
