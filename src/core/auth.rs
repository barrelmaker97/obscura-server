use crate::core::crypto_types::Signature;
use crate::error::{AppError, Result};
use argon2::{
    Argon2,
    password_hash::{PasswordHash, PasswordHasher, PasswordVerifier, SaltString},
};
use base64::Engine;
use jsonwebtoken::{DecodingKey, EncodingKey, Header, Validation, decode, encode};
use rand::{RngCore, rngs::OsRng};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::time::{SystemTime, UNIX_EPOCH};
use uuid::Uuid;
use xeddsa::Verify;
use ed25519_dalek::Verifier;

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

/// Verifies an XEdDSA signature.
///
/// XEdDSA is used to verify Ed25519 signatures against Curve25519 (Montgomery) public keys.
/// Because a Montgomery X-coordinate corresponds to two Edwards points (sign bit 0 and 1),
/// we try both to ensure compatibility with various client implementations and environments.
pub fn verify_signature(verifier_key: &crate::core::crypto_types::PublicKey, message: &[u8], signature: &Signature) -> Result<()> {
    let pk = xeddsa::xed25519::PublicKey(*verifier_key.as_crypto_bytes());

    use xeddsa::ConvertMont;
    let sig_bytes = signature.as_bytes();
    
    // We must clear the 255th bit of 's' for standard Ed25519 libraries.
    // XEdDSA uses this bit to represent the sign of the recovered point.
    let mut sig_canonical = *sig_bytes;
    sig_canonical[63] &= 0x7F;
    let sig_obj = ed25519_dalek::Signature::from_bytes(&sig_canonical);

    // Try both possible Edwards points for the Montgomery public key.
    // Some client environments (like JS polyfills) may choose the alternative point.
    for sign_bit in [0, 1] {
        if let Ok(ed_pk_bytes) = pk.convert_mont(sign_bit) {
            if let Ok(ed_pk) = ed25519_dalek::VerifyingKey::from_bytes(&ed_pk_bytes) {
                if ed_pk.verify(message, &sig_obj).is_ok() {
                    return Ok(());
                }
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
    use curve25519_dalek::edwards::CompressedEdwardsY;
    use xeddsa::xed25519::PrivateKey;
    use xeddsa::{CalculateKeyPair, Sign};

    #[test]
    fn test_verify_signature_simple() {
        use crate::core::crypto_types::PublicKey;

        // 1. Generate Identity Key (Verifier) using XEdDSA
        let mut ik_bytes = [0u8; 32];
        OsRng.fill_bytes(&mut ik_bytes);
        let private_key = PrivateKey(ik_bytes);
        
        // Calculate public key (using sign bit 0) - returns Edwards key
        let (_, ik_pub_ed) = private_key.calculate_key_pair(0);
        // Convert to Montgomery for verification
        let ik_pub_mont = CompressedEdwardsY(ik_pub_ed).decompress().unwrap().to_montgomery().to_bytes();
        let mut ik_pub_wire = [0u8; 33];
        ik_pub_wire[0] = 0x05;
        ik_pub_wire[1..].copy_from_slice(&ik_pub_mont);
        let ik_pub = PublicKey::new(ik_pub_wire);
        
        // 2. Message
        let message = b"hello world";

        // 3. Sign
        let signature_bytes: [u8; 64] = private_key.sign(message, OsRng);
        let signature = Signature::new(signature_bytes);

        // Verify with PublicKey object
        let res = verify_signature(&ik_pub, message, &signature);
        assert!(res.is_ok());
    }
}
