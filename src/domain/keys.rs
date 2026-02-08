use crate::domain::crypto::{PublicKey, Signature};
use crate::error::{AppError, Result};

#[derive(Debug, Clone)]
pub struct SignedPreKey {
    pub key_id: i32,
    pub public_key: PublicKey,
    pub signature: Signature,
}

#[derive(Debug, Clone)]
pub struct OneTimePreKey {
    pub key_id: i32,
    pub public_key: PublicKey,
}

impl OneTimePreKey {
    pub fn validate_uniqueness(keys: &[OneTimePreKey]) -> Result<()> {
        let mut unique_ids = std::collections::HashSet::with_capacity(keys.len());
        for pk in keys {
            if !unique_ids.insert(pk.key_id) {
                return Err(AppError::BadRequest(format!("Duplicate prekey ID: {}", pk.key_id)));
            }
        }
        Ok(())
    }
}

#[derive(Debug, Clone)]
pub struct PreKeyBundle {
    pub registration_id: i32,
    pub identity_key: PublicKey,
    pub signed_pre_key: SignedPreKey,
    pub one_time_pre_key: Option<OneTimePreKey>,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_validate_uniqueness() {
        let pk = PublicKey::new([0x05; 33]);
        let keys = vec![
            OneTimePreKey { key_id: 1, public_key: pk.clone() },
            OneTimePreKey { key_id: 2, public_key: pk.clone() },
        ];
        assert!(OneTimePreKey::validate_uniqueness(&keys).is_ok());

        let duplicate_keys = vec![
            OneTimePreKey { key_id: 1, public_key: pk.clone() },
            OneTimePreKey { key_id: 1, public_key: pk.clone() },
        ];
        assert!(OneTimePreKey::validate_uniqueness(&duplicate_keys).is_err());
    }
}
