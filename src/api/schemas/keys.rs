use crate::api::schemas::crypto::{PublicKey, Signature};
use crate::domain::crypto::{PublicKey as DomainPublicKey, Signature as DomainSignature};
use crate::domain::keys::{
    OneTimePreKey as DomainOneTimePreKey, PreKeyBundle as DomainPreKeyBundle, SignedPreKey as DomainSignedPreKey,
};
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SignedPreKey {
    pub key_id: i32,
    pub public_key: PublicKey,
    pub signature: Signature,
}

impl From<DomainSignedPreKey> for SignedPreKey {
    fn from(k: DomainSignedPreKey) -> Self {
        Self { key_id: k.key_id, public_key: k.public_key.into(), signature: k.signature.into() }
    }
}

impl TryFrom<SignedPreKey> for DomainSignedPreKey {
    type Error = String;
    fn try_from(schema: SignedPreKey) -> Result<Self, Self::Error> {
        Ok(Self {
            key_id: schema.key_id,
            public_key: DomainPublicKey::try_from(schema.public_key)?,
            signature: DomainSignature::try_from(schema.signature)?,
        })
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct OneTimePreKey {
    pub key_id: i32,
    pub public_key: PublicKey,
}

impl From<DomainOneTimePreKey> for OneTimePreKey {
    fn from(k: DomainOneTimePreKey) -> Self {
        Self { key_id: k.key_id, public_key: k.public_key.into() }
    }
}

impl TryFrom<OneTimePreKey> for DomainOneTimePreKey {
    type Error = String;
    fn try_from(schema: OneTimePreKey) -> Result<Self, Self::Error> {
        Ok(Self { key_id: schema.key_id, public_key: DomainPublicKey::try_from(schema.public_key)? })
    }
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PreKeyUpload {
    pub identity_key: Option<PublicKey>,
    pub registration_id: Option<i32>,
    pub signed_pre_key: SignedPreKey,
    pub one_time_pre_keys: Vec<OneTimePreKey>,
}

impl PreKeyUpload {
    /// Validates the pre-key upload payload.
    ///
    /// # Errors
    /// Returns an error if the registration ID is missing during a takeover or if there are duplicate pre-key IDs.
    pub fn validate(&self) -> Result<(), String> {
        if self.identity_key.is_some() && self.registration_id.is_none() {
            return Err("registrationId is required when identityKey is provided".into());
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
    use crate::api::schemas::crypto::PublicKey as SchemaPublicKey;

    fn mock_upload() -> PreKeyUpload {
        PreKeyUpload {
            identity_key: None,
            registration_id: None,
            signed_pre_key: SignedPreKey {
                key_id: 1,
                public_key: SchemaPublicKey("A".repeat(44)),
                signature: crate::api::schemas::crypto::Signature("B".repeat(88)),
            },
            one_time_pre_keys: vec![],
        }
    }

    #[test]
    fn test_upload_validation_standard_refill() {
        let upload = mock_upload();
        assert!(upload.validate().is_ok());
    }

    #[test]
    fn test_upload_validation_takeover_valid() {
        let mut upload = mock_upload();
        upload.identity_key = Some(SchemaPublicKey("C".repeat(44)));
        upload.registration_id = Some(456);
        assert!(upload.validate().is_ok());
    }

    #[test]
    fn test_upload_validation_takeover_missing_id() {
        let mut upload = mock_upload();
        upload.identity_key = Some(SchemaPublicKey("C".repeat(44)));
        upload.registration_id = None;
        let res = upload.validate();
        assert!(res.is_err());
        assert_eq!(res.unwrap_err(), "registrationId is required when identityKey is provided");
    }

    #[test]
    fn test_upload_validation_duplicate_keys() {
        let mut upload = mock_upload();
        upload.one_time_pre_keys = vec![
            OneTimePreKey { key_id: 1, public_key: SchemaPublicKey("A".repeat(44)) },
            OneTimePreKey { key_id: 1, public_key: SchemaPublicKey("B".repeat(44)) },
        ];
        let res = upload.validate();
        assert!(res.is_err());
        assert_eq!(res.unwrap_err(), "Duplicate prekey ID: 1");
    }
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct PreKeyBundle {
    pub registration_id: i32,
    pub identity_key: PublicKey,
    pub signed_pre_key: SignedPreKey,
    pub one_time_pre_key: Option<OneTimePreKey>,
}

impl From<DomainPreKeyBundle> for PreKeyBundle {
    fn from(b: DomainPreKeyBundle) -> Self {
        Self {
            registration_id: b.registration_id,
            identity_key: b.identity_key.into(),
            signed_pre_key: b.signed_pre_key.into(),
            one_time_pre_key: b.one_time_pre_key.map(Into::into),
        }
    }
}
