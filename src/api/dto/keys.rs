use crate::api::dto::crypto::{PublicKeyDto, SignatureDto};
use crate::domain::keys::{OneTimePreKey, SignedPreKey, PreKeyBundle};
use crate::domain::crypto::{PublicKey, Signature};
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SignedPreKeyDto {
    pub key_id: i32,
    pub public_key: PublicKeyDto,
    pub signature: SignatureDto,
}

impl From<SignedPreKey> for SignedPreKeyDto {
    fn from(k: SignedPreKey) -> Self {
        Self {
            key_id: k.key_id,
            public_key: k.public_key.into(),
            signature: k.signature.into(),
        }
    }
}

impl TryFrom<SignedPreKeyDto> for SignedPreKey {
    type Error = String;
    fn try_from(dto: SignedPreKeyDto) -> Result<Self, Self::Error> {
        Ok(Self {
            key_id: dto.key_id,
            public_key: PublicKey::try_from(dto.public_key)?,
            signature: Signature::try_from(dto.signature)?,
        })
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct OneTimePreKeyDto {
    pub key_id: i32,
    pub public_key: PublicKeyDto,
}

impl From<OneTimePreKey> for OneTimePreKeyDto {
    fn from(k: OneTimePreKey) -> Self {
        Self {
            key_id: k.key_id,
            public_key: k.public_key.into(),
        }
    }
}

impl TryFrom<OneTimePreKeyDto> for OneTimePreKey {
    type Error = String;
    fn try_from(dto: OneTimePreKeyDto) -> Result<Self, Self::Error> {
        Ok(Self {
            key_id: dto.key_id,
            public_key: PublicKey::try_from(dto.public_key)?,
        })
    }
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PreKeyUpload {
    pub identity_key: Option<PublicKeyDto>,
    pub registration_id: Option<i32>,
    pub signed_pre_key: SignedPreKeyDto,
    pub one_time_pre_keys: Vec<OneTimePreKeyDto>,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct PreKeyBundleDto {
    pub registration_id: i32,
    pub identity_key: PublicKeyDto,
    pub signed_pre_key: SignedPreKeyDto,
    pub one_time_pre_key: Option<OneTimePreKeyDto>,
}

impl From<PreKeyBundle> for PreKeyBundleDto {
    fn from(b: PreKeyBundle) -> Self {
        Self {
            registration_id: b.registration_id,
            identity_key: b.identity_key.into(),
            signed_pre_key: b.signed_pre_key.into(),
            one_time_pre_key: b.one_time_pre_key.map(Into::into),
        }
    }
}
