use crate::error::{AppError, Result};
use argon2::{
    Argon2,
    password_hash::{PasswordHash, PasswordHasher, PasswordVerifier, SaltString},
};
use base64::Engine;
use ed25519_dalek::{Signature, Verifier, VerifyingKey};
use rand::{RngCore, rngs::OsRng};
use sha2::{Digest, Sha256};

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
