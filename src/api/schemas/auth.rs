use crate::api::schemas::crypto::PublicKey;
use crate::api::schemas::keys::{OneTimePreKey, SignedPreKey};
use serde::{Deserialize, Serialize};

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Registration {
    pub username: String,
    pub password: String,
    pub identity_key: PublicKey,
    pub registration_id: i32,
    pub signed_pre_key: SignedPreKey,
    pub one_time_pre_keys: Vec<OneTimePreKey>,
}

impl Registration {
    /// Validates the registration payload.
    ///
    /// # Errors
    /// Returns an error if the password is too short or if there are duplicate pre-key IDs.
    pub fn validate(&self) -> Result<(), String> {
        if self.password.len() < 12 {
            return Err("Password must be at least 12 characters long".into());
        }

        let mut unique_ids = std::collections::HashSet::with_capacity(self.one_time_pre_keys.len());
        for pk in &self.one_time_pre_keys {
            if !unique_ids.insert(pk.key_id) {
                return Err(format!("Duplicate prekey ID: {}", pk.key_id));
            }
        }

        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::api::schemas::crypto::{PublicKey as SchemaPublicKey, Signature};
    use crate::api::schemas::keys::{OneTimePreKey, SignedPreKey};

    fn mock_registration(password: &str) -> Registration {
        Registration {
            username: "testuser".into(),
            password: password.into(),
            identity_key: SchemaPublicKey("A".repeat(44)), // Dummy B64
            registration_id: 123,
            signed_pre_key: SignedPreKey {
                key_id: 1,
                public_key: SchemaPublicKey("B".repeat(44)),
                signature: Signature("C".repeat(88)),
            },
            one_time_pre_keys: vec![],
        }
    }

    #[test]
    fn test_registration_validation_valid() {
        let reg = mock_registration("password12345");
        assert!(reg.validate().is_ok());
    }

    #[test]
    fn test_registration_validation_too_short() {
        let reg = mock_registration("short");
        let res = reg.validate();
        assert!(res.is_err());
        assert_eq!(res.unwrap_err(), "Password must be at least 12 characters long");
    }

    #[test]
    fn test_registration_validation_duplicate_keys() {
        let mut reg = mock_registration("password12345");
        reg.one_time_pre_keys = vec![
            OneTimePreKey { key_id: 1, public_key: SchemaPublicKey("A".repeat(44)) },
            OneTimePreKey { key_id: 1, public_key: SchemaPublicKey("B".repeat(44)) },
        ];
        let res = reg.validate();
        assert!(res.is_err());
        assert_eq!(res.unwrap_err(), "Duplicate prekey ID: 1");
    }
}

#[derive(Deserialize)]
pub struct Login {
    pub username: String,
    pub password: String,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Refresh {
    pub refresh_token: String,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Logout {
    pub refresh_token: String,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct AuthSession {
    pub token: String,
    pub refresh_token: String,
    pub expires_at: i64,
}
