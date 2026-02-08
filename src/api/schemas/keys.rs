use crate::api::schemas::crypto::{PublicKey, Signature};
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SignedPreKey {
    pub key_id: i32,
    pub public_key: PublicKey,
    pub signature: Signature,
}

impl From<crate::domain::keys::SignedPreKey> for SignedPreKey {
    fn from(k: crate::domain::keys::SignedPreKey) -> Self {
        Self {
            key_id: k.key_id,
            public_key: k.public_key.into(),
            signature: k.signature.into(),
        }
    }
}

impl TryFrom<SignedPreKey> for crate::domain::keys::SignedPreKey {
    type Error = String;
    fn try_from(schema: SignedPreKey) -> Result<Self, Self::Error> {
        Ok(Self {
            key_id: schema.key_id,
            public_key: crate::domain::crypto::PublicKey::try_from(schema.public_key)?,
            signature: crate::domain::crypto::Signature::try_from(schema.signature)?,
        })
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct OneTimePreKey {
    pub key_id: i32,
    pub public_key: PublicKey,
}

impl From<crate::domain::keys::OneTimePreKey> for OneTimePreKey {
    fn from(k: crate::domain::keys::OneTimePreKey) -> Self {
        Self {
            key_id: k.key_id,
            public_key: k.public_key.into(),
        }
    }
}

impl TryFrom<OneTimePreKey> for crate::domain::keys::OneTimePreKey {
    type Error = String;
    fn try_from(schema: OneTimePreKey) -> Result<Self, Self::Error> {
        Ok(Self {
            key_id: schema.key_id,
            public_key: crate::domain::crypto::PublicKey::try_from(schema.public_key)?,
        })
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

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct PreKeyBundle {
    pub registration_id: i32,
    pub identity_key: PublicKey,
    pub signed_pre_key: SignedPreKey,
    pub one_time_pre_key: Option<OneTimePreKey>,
}

impl From<crate::domain::keys::PreKeyBundle> for PreKeyBundle {
    fn from(b: crate::domain::keys::PreKeyBundle) -> Self {
        Self {
            registration_id: b.registration_id,
            identity_key: b.identity_key.into(),
            signed_pre_key: b.signed_pre_key.into(),
            one_time_pre_key: b.one_time_pre_key.map(Into::into),
        }
    }
}
