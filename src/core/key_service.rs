use crate::config::Config;
use crate::core::auth::verify_signature;
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

        let is_takeover = self.upload_keys_internal(&mut tx, params).await?;

        tx.commit().await?;

        if is_takeover {
            self.notifier.notify(user_id, UserEvent::Disconnect);
        }

        Ok(())
    }

    /// Internal implementation that accepts a mutable connection.
    pub async fn upload_keys_internal(&self, conn: &mut PgConnection, params: KeyUploadParams) -> Result<bool> {
        let mut is_takeover = false;

        // 1. Identify/Verify Identity Key
        let ik_bytes = if let Some(new_ik_bytes) = &params.identity_key {
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
            new_ik_bytes.clone()
        } else {
            self.key_repo
                .fetch_identity_key_for_update(&mut *conn, params.user_id)
                .await?
                .ok_or_else(|| AppError::BadRequest("Identity key missing".into()))?
        };

        // 2. Verify Cryptographic Signature
        // The signature is expected to be an Ed25519 signature of the Signed Prekey's Public Key,
        // signed by the user's Identity Key.
        verify_keys(&ik_bytes, &params.signed_pre_key)?;

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

            // identity_key is Some if is_takeover is true (from logic in step 1)
            if let Some(ik) = &params.identity_key {
                self.key_repo.upsert_identity_key(&mut *conn, params.user_id, ik, reg_id).await?;
            }
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

        let otpk_vec: Vec<(i32, Vec<u8>)> =
            params.one_time_pre_keys.into_iter().map(|k| (k.key_id, k.public_key)).collect();

        self.key_repo.insert_one_time_pre_keys(&mut *conn, params.user_id, &otpk_vec).await?;

        Ok(is_takeover)
    }
}

fn verify_keys(ik_bytes: &[u8], signed_pre_key: &SignedPreKey) -> Result<()> {
    // NOTE: Libsignal clients often upload keys with a 0x05 type byte (33 bytes).
    
    // The Identity Key MUST be 32 bytes for the verifier instantiation (Ed25519 specific).
    // We strictly handle the 0x05 wrapper for this specific key type.
    let ik_raw = if ik_bytes.len() == 33 { &ik_bytes[1..] } else { ik_bytes };

    // The Signed Pre Key Public Key is the MESSAGE.
    // Standard libsignal clients (typescript) appear to sign the stripped 32-byte key, 
    // even though they upload the 33-byte key.
    // Other clients might sign the full 33-byte key.
    // We try both to be robust.

    // 1. Try verifying the exact public key provided
    if verify_signature(ik_raw, &signed_pre_key.public_key, &signed_pre_key.signature).is_ok() {
        return Ok(());
    }

    // 2. Fallback: If 33 bytes, try verifying the stripped 32-byte key
    if signed_pre_key.public_key.len() == 33 {
        let spk_pub_raw = &signed_pre_key.public_key[1..];
        if verify_signature(ik_raw, spk_pub_raw, &signed_pre_key.signature).is_ok() {
            return Ok(());
        }
    }

    Err(AppError::BadRequest("Invalid signature".into()))
}

#[cfg(test)]
mod tests {
    use super::*;
    use ed25519_dalek::{Signer, SigningKey};
    use rand::RngCore;
    use rand::rngs::OsRng;

    fn generate_keys() -> (SigningKey, Vec<u8>, SigningKey, Vec<u8>, Vec<u8>) {
        let mut ik_bytes = [0u8; 32];
        OsRng.fill_bytes(&mut ik_bytes);
        let ik = SigningKey::from_bytes(&ik_bytes);
        let ik_pub = ik.verifying_key().to_bytes().to_vec();

        let mut spk_bytes = [0u8; 32];
        OsRng.fill_bytes(&mut spk_bytes);
        let spk = SigningKey::from_bytes(&spk_bytes);
        let spk_pub = spk.verifying_key().to_bytes().to_vec();

        let signature = ik.sign(&spk_pub).to_bytes().to_vec();

        (ik, ik_pub, spk, spk_pub, signature)
    }

    #[test]
    fn test_verify_keys_standard() {
        let (_, ik_pub, _, spk_pub, signature) = generate_keys();
        let spk = SignedPreKey { key_id: 1, public_key: spk_pub, signature };

        assert!(verify_keys(&ik_pub, &spk).is_ok());
    }

    #[test]
    fn test_verify_keys_strict_33() {
        // Client signs the 33-byte key (Explicit strictness)
        let mut ik_bytes = [0u8; 32];
        OsRng.fill_bytes(&mut ik_bytes);
        let ik = SigningKey::from_bytes(&ik_bytes);
        let mut ik_pub_33 = ik.verifying_key().to_bytes().to_vec();
        ik_pub_33.insert(0, 0x05);

        let mut spk_bytes = [0u8; 32];
        OsRng.fill_bytes(&mut spk_bytes);
        let spk = SigningKey::from_bytes(&spk_bytes);
        
        let mut spk_pub_33 = spk.verifying_key().to_bytes().to_vec();
        spk_pub_33.insert(0, 0x05); 

        // Sign 33 bytes
        let signature = ik.sign(&spk_pub_33).to_bytes().to_vec();

        let spk = SignedPreKey { key_id: 1, public_key: spk_pub_33, signature };

        assert!(verify_keys(&ik_pub_33, &spk).is_ok());
    }

    #[test]
    fn test_verify_keys_libsignal_behavior() {
        // Client sends 33-byte key but signs the 32-byte raw key (Libsignal default)
        let mut ik_bytes = [0u8; 32];
        OsRng.fill_bytes(&mut ik_bytes);
        let ik = SigningKey::from_bytes(&ik_bytes);
        let mut ik_pub_33 = ik.verifying_key().to_bytes().to_vec();
        ik_pub_33.insert(0, 0x05);

        let mut spk_bytes = [0u8; 32];
        OsRng.fill_bytes(&mut spk_bytes);
        let spk = SigningKey::from_bytes(&spk_bytes);
        let spk_pub_32 = spk.verifying_key().to_bytes().to_vec();
        
        let mut spk_pub_33 = spk_pub_32.clone();
        spk_pub_33.insert(0, 0x05); 

        // Sign 32 bytes (Raw)
        let signature = ik.sign(&spk_pub_32).to_bytes().to_vec();

        // Send 33 bytes
        let spk = SignedPreKey { key_id: 1, public_key: spk_pub_33, signature };

        assert!(verify_keys(&ik_pub_33, &spk).is_ok());
    }

    #[test]
    fn test_verify_keys_mixed() {
        let (_, ik_pub, _, spk_pub, signature) = generate_keys();

        let mut ik_pub_33 = ik_pub.clone();
        ik_pub_33.insert(0, 0x05);

        let spk = SignedPreKey { key_id: 1, public_key: spk_pub, signature };

        // 33-byte Identity Key, 32-byte SPK -> OK
        assert!(verify_keys(&ik_pub_33, &spk).is_ok());
    }
}
