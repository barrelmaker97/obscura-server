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
use time::OffsetDateTime;
use uuid::Uuid;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RefreshToken {
    pub token_hash: String,
    pub user_id: Uuid,
    pub expires_at: OffsetDateTime,
    pub created_at: OffsetDateTime,
}

impl RefreshToken {
    pub fn is_expired(&self) -> bool {
        self.expires_at < OffsetDateTime::now_utc()
    }
}

#[derive(Debug, Serialize, Deserialize, PartialEq, Eq)]
pub struct Claims {
    pub sub: Uuid,
    pub exp: usize,
}

impl Claims {
    pub fn new(user_id: Uuid, ttl_secs: u64) -> Self {
        let expiration = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap_or(std::time::Duration::from_secs(0))
            .as_secs() as usize
            + ttl_secs as usize;
        
        Self { sub: user_id, exp: expiration }
    }

    pub fn encode(&self, secret: &str) -> Result<String> {
        encode(
            &Header::default(),
            self,
            &EncodingKey::from_secret(secret.as_bytes()),
        )
        .map_err(|_| AppError::Internal)
    }

    pub fn decode(token: &str, secret: &str) -> Result<Self> {
        let token_data = decode::<Claims>(
            token,
            &DecodingKey::from_secret(secret.as_bytes()),
            &Validation::default(),
        )
        .map_err(|_| AppError::AuthError)?;
        
        Ok(token_data.claims)
    }
}

pub struct Password;

impl Password {
    #[tracing::instrument(skip(password), level = "debug")]
    pub fn hash(password: &str) -> Result<String> {
        let salt = SaltString::generate(&mut OsRng);
        let argon2 = Argon2::default();
        let password_hash = argon2.hash_password(password.as_bytes(), &salt).map_err(|_| AppError::Internal)?.to_string();
        Ok(password_hash)
    }

    pub fn verify(password: &str, hash: &str) -> Result<bool> {
        let parsed_hash = PasswordHash::new(hash).map_err(|_| AppError::Internal)?;
        Ok(Argon2::default().verify_password(password.as_bytes(), &parsed_hash).is_ok())
    }
}

pub struct OpaqueToken;

impl OpaqueToken {
    /// Generates a cryptographically secure random string (32 bytes -> Base64).
    pub fn generate() -> String {
        let mut bytes = [0u8; 32];
        OsRng.fill_bytes(&mut bytes);
        base64::engine::general_purpose::URL_SAFE_NO_PAD.encode(bytes)
    }

    /// Hashes a token using SHA-256 for secure storage.
    pub fn hash(token: &str) -> String {
        let mut hasher = Sha256::new();
        hasher.update(token.as_bytes());
        hex::encode(hasher.finalize())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_claims_roundtrip() {
        let user_id = Uuid::new_v4();
        let secret = "test_secret";
        let claims = Claims::new(user_id, 3600);
        
        let token = claims.encode(secret).unwrap();
        let decoded = Claims::decode(&token, secret).unwrap();
        
        assert_eq!(claims, decoded);
    }

    #[test]
    fn test_claims_invalid_secret() {
        let user_id = Uuid::new_v4();
        let claims = Claims::new(user_id, 3600);
        let token = claims.encode("secret1").unwrap();
        
        let result = Claims::decode(&token, "secret2");
        assert!(matches!(result, Err(AppError::AuthError)));
    }

    #[test]
    fn test_password_hashing() {
        let password = "password12345";
        let hash = Password::hash(password).unwrap();
        
        assert!(Password::verify(password, &hash).unwrap());
        assert!(!Password::verify("wrong_password", &hash).unwrap());
    }

    #[test]
    fn test_opaque_token_generation() {
        let token1 = OpaqueToken::generate();
        let token2 = OpaqueToken::generate();
        
        assert_ne!(token1, token2);
        assert_eq!(token1.len(), 43); // 32 bytes Base64 no pad
    }

    #[test]
    fn test_opaque_token_hashing() {
        let token = "my_token";
        let hash1 = OpaqueToken::hash(token);
        let hash2 = OpaqueToken::hash(token);
        
        assert_eq!(hash1, hash2);
        assert_ne!(token, hash1);
    }
}