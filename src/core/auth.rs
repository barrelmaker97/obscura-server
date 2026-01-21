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

#[cfg(test)]
mod tests {
    use super::*;
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
        spk_pub_33.insert(0, 0x05); // Add DJB type byte

        // Case 1: Verify using 32-byte message -> Should Pass
        let res1 = verify_signature(&ik_pub, &spk_pub_32, &signature);
        assert!(res1.is_ok(), "Case 1 failed: Standard 32-byte verification");

        // Case 2: Verify using 33-byte message -> Should FAIL (Strictness check)
        let res2 = verify_signature(&ik_pub, &spk_pub_33, &signature);
        assert!(res2.is_err(), "Case 2 failed: verify_signature should NOT accept 33-byte messages implicitly");

        // Case 3: Verify using 33-byte public key (verifier) -> Should FAIL (Strictness check)
        let mut ik_pub_33 = ik_pub.clone();
        ik_pub_33.insert(0, 0x05);
        let res3 = verify_signature(&ik_pub_33, &spk_pub_32, &signature);
        assert!(res3.is_err(), "Case 3 failed: verify_signature should NOT accept 33-byte verifier keys implicitly");
    }
}
