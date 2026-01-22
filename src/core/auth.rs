use crate::error::{AppError, Result};
use argon2::{
    Argon2,
    password_hash::{PasswordHash, PasswordHasher, PasswordVerifier, SaltString},
};
use base64::Engine;
use curve25519_dalek::montgomery::MontgomeryPoint;
use ed25519_dalek::{Signature, Verifier, VerifyingKey};
use jsonwebtoken::{DecodingKey, EncodingKey, Header, Validation, decode, encode};
use rand::{RngCore, rngs::OsRng};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::time::{SystemTime, UNIX_EPOCH};
use uuid::Uuid;

/// Bitmask to clear the XEdDSA sign bit (the 255th bit of the scalar 's').
pub const XEDDSA_SIGN_BIT_MASK: u8 = 0x7F;
/// The XEdDSA sign bit itself.
pub const XEDDSA_SIGN_BIT: u8 = 0x80;

#[derive(Debug, Serialize, Deserialize)]
pub struct Claims {
    pub sub: Uuid,
    pub exp: usize,
}

pub fn create_jwt(user_id: Uuid, secret: &str, ttl_secs: u64) -> Result<String> {
    let expiration = SystemTime::now().duration_since(UNIX_EPOCH).unwrap().as_secs() as usize + ttl_secs as usize;

    let claims = Claims { sub: user_id, exp: expiration };

    encode(&Header::default(), &claims, &EncodingKey::from_secret(secret.as_bytes())).map_err(|_| AppError::Internal)
}

pub fn verify_jwt(token: &str, secret: &str) -> Result<Claims> {
    let token_data = decode::<Claims>(token, &DecodingKey::from_secret(secret.as_bytes()), &Validation::default())
        .map_err(|_| AppError::AuthError)?;
    Ok(token_data.claims)
}

pub fn hash_password(password: &str) -> Result<String> {
    let salt = SaltString::generate(&mut OsRng);
    let argon2 = Argon2::default();
    let password_hash = argon2.hash_password(password.as_bytes(), &salt).map_err(|_| AppError::Internal)?.to_string();
    Ok(password_hash)
}

pub fn verify_password(password: &str, password_hash: &str) -> Result<bool> {
    let parsed_hash = PasswordHash::new(password_hash).map_err(|_| AppError::Internal)?;
    Ok(Argon2::default().verify_password(password.as_bytes(), &parsed_hash).is_ok())
}

/// Verifies an Ed25519 signature.
pub fn verify_signature(public_key_bytes: &[u8], message: &[u8], signature_bytes: &[u8]) -> Result<()> {
    let public_key = VerifyingKey::from_bytes(
        public_key_bytes.try_into().map_err(|_| AppError::BadRequest("Invalid public key length".into()))?,
    )
    .map_err(|_| AppError::BadRequest("Invalid public key".into()))?;

    let signature = Signature::from_bytes(
        signature_bytes.try_into().map_err(|_| AppError::BadRequest("Invalid signature length".into()))?,
    );

    public_key.verify(message, &signature).map_err(|_| AppError::BadRequest("Invalid signature".into()))?;

    Ok(())
}

/// Verifies an Ed25519 signature using a Montgomery (Curve25519) public key by converting it.
pub fn verify_signature_with_montgomery(
    public_key_bytes: &[u8],
    message: &[u8],
    signature_bytes: &[u8],
) -> Result<()> {
    let mont_bytes: [u8; 32] =
        public_key_bytes.try_into().map_err(|_| AppError::BadRequest("Invalid public key length".into()))?;
    let mont_point = MontgomeryPoint(mont_bytes);

    // XEdDSA signatures (as used by Signal) store a sign bit in the 255th bit of 's'.
    // Standard Ed25519 (and ed25519_dalek) expect 's' to be a canonical scalar < L.
    // We must clear this bit before verification if we are using an Ed25519 library.
    let mut signature_bytes_fixed: [u8; 64] = signature_bytes.try_into().map_err(|_| AppError::BadRequest("Invalid signature length".into()))?;
    signature_bytes_fixed[63] &= XEDDSA_SIGN_BIT_MASK;

    let signature = Signature::from_bytes(&signature_bytes_fixed);

    tracing::debug!("verify_signature_with_montgomery: message_len={}, signature_len={}", message.len(), signature_bytes.len());

    // XEd25519 conversion has a sign ambiguity. One Montgomery point corresponds to two Edwards points (P and -P).
    // Try converting with sign 0 first (standard XEd25519).
    if let Some(ed_point) = mont_point.to_edwards(0) {
        let ed_bytes = ed_point.compress().to_bytes();
        if let Ok(public_key) = VerifyingKey::from_bytes(&ed_bytes) {
            if public_key.verify(message, &signature).is_ok() {
                return Ok(());
            }
        }
    }

    // If that fails, try sign 1.
    if let Some(ed_point) = mont_point.to_edwards(1) {
        let ed_bytes = ed_point.compress().to_bytes();
        if let Ok(public_key) = VerifyingKey::from_bytes(&ed_bytes) {
            if public_key.verify(message, &signature).is_ok() {
                return Ok(());
            }
        }
    }

    Err(AppError::BadRequest("Invalid signature".into()))
}

/// Generates a cryptographically secure random string (32 bytes -> Base64).
pub fn generate_opaque_token() -> String {
    let mut bytes = [0u8; 32];
    OsRng.fill_bytes(&mut bytes);
    base64::engine::general_purpose::URL_SAFE_NO_PAD.encode(bytes)
}

/// Hashes a token using SHA-256 for secure storage.
pub fn hash_token(token: &str) -> String {
    let mut hasher = Sha256::new();
    hasher.update(token.as_bytes());
    hex::encode(hasher.finalize())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::core::crypto_types::DJB_KEY_PREFIX;
    use ed25519_dalek::{Signer, SigningKey};

    #[test]
    fn test_verify_signature_strictness() {
        // 1. Generate Identity Key (Verifier)
        let mut ik_bytes = [0u8; 32];
        OsRng.fill_bytes(&mut ik_bytes);
        let identity_key = SigningKey::from_bytes(&ik_bytes);
        let ik_pub = identity_key.verifying_key().to_bytes().to_vec();

        // 2. Generate Signed Pre Key (Message)
        let mut spk_bytes = [0u8; 32];
        OsRng.fill_bytes(&mut spk_bytes);
        let spk_key = SigningKey::from_bytes(&spk_bytes);
        let spk_pub_32 = spk_key.verifying_key().to_bytes().to_vec();

        // 3. Sign the 32-byte SPK public key
        let signature = identity_key.sign(&spk_pub_32).to_bytes().to_vec();

        // 4. Create 33-byte versions
        let mut spk_pub_33 = spk_pub_32.clone();
        spk_pub_33.insert(0, DJB_KEY_PREFIX); // Add DJB type byte

        // Case 1: Verify using 32-byte message -> Should Pass
        let res1 = verify_signature(&ik_pub, &spk_pub_32, &signature);
        assert!(res1.is_ok(), "Case 1 failed: Standard 32-byte verification");

        // Case 2: Verify using 33-byte message -> Should FAIL (Strictness check)
        let res2 = verify_signature(&ik_pub, &spk_pub_33, &signature);
        assert!(res2.is_err(), "Case 2 failed: verify_signature should NOT accept 33-byte messages implicitly");

        // Case 3: Verify using 33-byte public key (verifier) -> Should FAIL (Strictness check)
        let mut ik_pub_33 = ik_pub.clone();
        ik_pub_33.insert(0, DJB_KEY_PREFIX);
        let res3 = verify_signature(&ik_pub_33, &spk_pub_32, &signature);
        assert!(res3.is_err(), "Case 3 failed: verify_signature should NOT accept 33-byte verifier keys implicitly");
    }

    #[test]
    fn test_verify_signature_with_high_bit_set() {
        use curve25519_dalek::edwards::CompressedEdwardsY;

        // 1. Generate Identity Key (Verifier)
        let mut ik_bytes = [0u8; 32];
        OsRng.fill_bytes(&mut ik_bytes);
        let identity_key = SigningKey::from_bytes(&ik_bytes);
        let ik_pub_ed = identity_key.verifying_key().to_bytes();

        // Convert Ed25519 Public Key (Edwards) -> X25519 Public Key (Montgomery)
        let ed_point = CompressedEdwardsY(ik_pub_ed).decompress().unwrap();
        let mont_point = ed_point.to_montgomery();
        let ik_pub_x25519 = mont_point.to_bytes();

        // 2. Generate Message
        let message = b"test message";

        // 3. Sign
        let mut signature_bytes = identity_key.sign(message).to_bytes();

        // 4. Force non-canonical by setting high bit (simulating XEdDSA)
        signature_bytes[63] |= XEDDSA_SIGN_BIT;

        // 5. Verify using Montgomery path
        let res = verify_signature_with_montgomery(&ik_pub_x25519, message, &signature_bytes);
        assert!(res.is_ok(), "Should verify signature even with high bit set by clearing it internally");
    }
}
