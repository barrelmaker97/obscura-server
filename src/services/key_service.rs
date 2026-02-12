use crate::config::MessagingConfig;
use crate::domain::crypto::PublicKey;
use crate::domain::keys::{OneTimePreKey, PreKeyBundle, SignedPreKey};
use crate::error::{AppError, Result};
use crate::proto::obscura::v1::PreKeyStatus;
use crate::services::crypto_service::CryptoService;
use crate::storage::DbPool;
use crate::storage::key_repo::KeyRepository;
use opentelemetry::{global, metrics::Counter};
use sqlx::PgConnection;
use uuid::Uuid;

#[derive(Clone, Debug)]
struct Metrics {
    prekey_low_total: Counter<u64>,
}

impl Metrics {
    fn new() -> Self {
        let meter = global::meter("obscura-server");
        Self {
            prekey_low_total: meter
                .u64_counter("keys_prekey_low_total")
                .with_description("Events where users dipped below prekey threshold")
                .build(),
        }
    }
}

#[derive(Clone, Debug)]
pub struct KeyService {
    pool: DbPool,
    repo: KeyRepository,
    crypto_service: CryptoService,
    config: MessagingConfig,
    metrics: Metrics,
}

#[derive(Debug)]
pub struct KeyUploadParams {
    pub user_id: Uuid,
    pub identity_key: Option<PublicKey>,
    pub registration_id: Option<i32>,
    pub signed_pre_key: SignedPreKey,
    pub one_time_pre_keys: Vec<OneTimePreKey>,
}

impl KeyService {
    #[must_use]
    pub fn new(pool: DbPool, repo: KeyRepository, crypto_service: CryptoService, config: MessagingConfig) -> Self {
        Self { pool, repo, crypto_service, config, metrics: Metrics::new() }
    }

    /// Fetches a pre-key bundle for a user.
    ///
    /// # Errors
    /// Returns `AppError::Database` if the database operation fails.
    #[tracing::instrument(err, skip(self), fields(user_id = %user_id))]
    pub async fn get_pre_key_bundle(&self, user_id: Uuid) -> Result<Option<PreKeyBundle>> {
        let mut conn = self.pool.acquire().await?;
        self.repo.fetch_pre_key_bundle(&mut conn, user_id).await
    }

    /// Fetches the identity key for a user.
    ///
    /// # Errors
    /// Returns `AppError::Database` if the database operation fails.
    #[tracing::instrument(err, skip(self), fields(user_id = %user_id))]
    pub async fn fetch_identity_key(&self, user_id: Uuid) -> Result<Option<PublicKey>> {
        let mut conn = self.pool.acquire().await?;
        self.repo.fetch_identity_key(&mut conn, user_id).await
    }

    /// Checks if a user needs to refill their one-time pre-keys.
    ///
    /// # Errors
    /// Returns `AppError::Database` if the database operation fails.
    #[tracing::instrument(err, skip(self), fields(user_id = %user_id))]
    pub async fn check_pre_key_status(&self, user_id: Uuid) -> Result<Option<PreKeyStatus>> {
        let mut conn = self.pool.acquire().await?;
        let count = self.repo.count_one_time_pre_keys(&mut conn, user_id).await?;
        if count < i64::from(self.config.pre_key_refill_threshold) {
            self.metrics.prekey_low_total.add(1, &[]);

            Ok(Some(PreKeyStatus {
                one_time_pre_key_count: i32::try_from(count).unwrap_or(i32::MAX),
                min_threshold: self.config.pre_key_refill_threshold,
            }))
        } else {
            Ok(None)
        }
    }

    /// Internal implementation that accepts a mutable connection.
    #[tracing::instrument(level = "debug", skip(self, conn, params), err(level = "debug"))]
    pub(crate) async fn upsert_keys(&self, conn: &mut PgConnection, params: KeyUploadParams) -> Result<bool> {
        let mut is_takeover = false;

        // 1. Identify/Verify Identity Key
        let ik = if let Some(new_ik) = params.identity_key {
            // Fetch existing identity key with LOCK
            let existing_ik_opt = self.repo.fetch_identity_key_for_update(&mut *conn, params.user_id).await?;

            if let Some(existing_ik) = existing_ik_opt {
                if existing_ik != new_ik {
                    tracing::info!("Device takeover detected: identity key has changed");
                    is_takeover = true;
                }
            } else {
                tracing::info!("Device takeover detected: new identity key for existing user");
                is_takeover = true;
            }

            self.verify_keys(&new_ik, &params.signed_pre_key)?;
            new_ik
        } else {
            // Must exist
            let ik = self
                .repo
                .fetch_identity_key_for_update(&mut *conn, params.user_id)
                .await?
                .ok_or_else(|| AppError::BadRequest("Identity key missing".into()))?;

            // Verify signature with the stored key
            self.verify_keys(&ik, &params.signed_pre_key)?;
            ik
        };

        // 2. Monotonic ID Check (Prevent Replay / Rollback)
        if !is_takeover {
            let max_id = self.repo.find_max_signed_pre_key_id(&mut *conn, params.user_id).await?;
            if let Some(current_max) = max_id
                && params.signed_pre_key.key_id <= current_max
            {
                return Err(AppError::BadRequest(format!(
                    "Signed Pre-Key ID {} must be greater than current ID {}",
                    params.signed_pre_key.key_id, current_max
                )));
            }
        }

        // 3. Limit Check (Atomic within transaction)
        let current_count =
            if is_takeover { 0 } else { self.repo.count_one_time_pre_keys(&mut *conn, params.user_id).await? };

        let new_keys_count = i64::try_from(params.one_time_pre_keys.len()).unwrap_or(i64::MAX);

        if new_keys_count > self.config.max_pre_keys {
            return Err(AppError::BadRequest(format!("Batch too large. Limit is {}", self.config.max_pre_keys)));
        }

        // 4. Handle Takeover Cleanup
        if is_takeover {
            let reg_id =
                params.registration_id.expect("registration_id must be present for takeover (validated at boundary)");

            self.repo.delete_all_signed_pre_keys(&mut *conn, params.user_id).await?;
            self.repo.delete_all_one_time_pre_keys(&mut *conn, params.user_id).await?;

            // Note: Message deletion and notification are now handled by the orchestrator.

            // Upsert Identity Key
            self.repo.upsert_identity_key(&mut *conn, params.user_id, &ik, reg_id).await?;
        } else {
            // If not a takeover, we might need to prune old keys to make room for new ones
            if current_count + new_keys_count > self.config.max_pre_keys {
                let to_delete = (current_count + new_keys_count) - self.config.max_pre_keys;
                self.repo.delete_oldest_one_time_pre_keys(&mut *conn, params.user_id, to_delete).await?;
            }
        }

        // 5. Common flow: Upsert Keys
        self.repo
            .upsert_signed_pre_key(
                &mut *conn,
                params.user_id,
                params.signed_pre_key.key_id,
                &params.signed_pre_key.public_key,
                &params.signed_pre_key.signature,
            )
            .await?;

        // 6. Cleanup old Signed Pre-Keys
        if !is_takeover {
            self.repo
                .delete_signed_pre_keys_older_than(&mut *conn, params.user_id, params.signed_pre_key.key_id)
                .await?;
        }

        self.repo.insert_one_time_pre_keys(&mut *conn, params.user_id, &params.one_time_pre_keys).await?;

        Ok(is_takeover)
    }

    fn verify_keys(&self, ik: &PublicKey, signed_pre_key: &SignedPreKey) -> Result<()> {
        let raw_32 = signed_pre_key.public_key.as_crypto_bytes();
        let wire_33 = signed_pre_key.public_key.as_bytes();

        // libsignal-protocol-typescript's generateSignedPreKey signs the 33-byte publicKey ArrayBuffer.
        // However, some versions or test polyfills might sign the 32-byte raw key.
        // We try both to be absolutely robust.

        if self.crypto_service.verify_signature(ik, wire_33, &signed_pre_key.signature).is_ok() {
            return Ok(());
        }

        self.crypto_service.verify_signature(ik, raw_32, &signed_pre_key.signature)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::domain::crypto::{DJB_KEY_PREFIX, PublicKey, Signature};
    use curve25519_dalek::edwards::CompressedEdwardsY;
    use rand::RngCore;
    use rand::rngs::OsRng;
    use xeddsa::xed25519::PrivateKey;
    use xeddsa::{CalculateKeyPair, Sign};

    fn setup_service() -> KeyService {
        let config = MessagingConfig::default();
        let pool = sqlx::PgPool::connect_lazy("postgres://localhost").expect("Valid test pool");
        KeyService::new(pool, KeyRepository::new(), CryptoService::new(), config)
    }

    fn generate_keys() -> (PrivateKey, PublicKey, PrivateKey, PublicKey, Signature) {
        let mut ik_bytes = [0u8; 32];
        OsRng.fill_bytes(&mut ik_bytes);
        let ik = PrivateKey(ik_bytes);

        let (_, ik_pub_ed) = ik.calculate_key_pair(0);
        let ik_pub_mont = CompressedEdwardsY(ik_pub_ed)
            .decompress()
            .expect("Failed to decompress")
            .to_montgomery()
            .to_bytes();
        let mut ik_pub_wire = [0u8; 33];
        ik_pub_wire[0] = DJB_KEY_PREFIX;
        ik_pub_wire[1..].copy_from_slice(&ik_pub_mont);
        let ik_pub = PublicKey::new(ik_pub_wire);

        let mut spk_bytes = [0u8; 32];
        OsRng.fill_bytes(&mut spk_bytes);
        let spk = PrivateKey(spk_bytes);
        let (_, spk_pub_ed) = spk.calculate_key_pair(0);
        let spk_pub_mont = CompressedEdwardsY(spk_pub_ed)
            .decompress()
            .expect("Failed to decompress")
            .to_montgomery()
            .to_bytes();
        let mut spk_pub_wire = [0u8; 33];
        spk_pub_wire[0] = DJB_KEY_PREFIX;
        spk_pub_wire[1..].copy_from_slice(&spk_pub_mont);
        let spk_pub = PublicKey::new(spk_pub_wire);

        // Sign the 33-byte wire format (prefix + raw X25519)
        let signature_bytes: [u8; 64] = ik.sign(spk_pub_wire.as_slice(), OsRng);
        let signature = Signature::new(signature_bytes);

        (ik, ik_pub, spk, spk_pub, signature)
    }

    #[tokio::test]
    async fn test_verify_keys_client_format() {
        let service = setup_service();
        let (_, ik_pub, _, spk_pub, signature) = generate_keys();
        let spk = SignedPreKey { key_id: 1, public_key: spk_pub, signature };

        assert!(service.verify_keys(&ik_pub, &spk).is_ok());
    }
}
