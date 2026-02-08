use serde::{Deserialize, Serialize};
use crate::domain::crypto::{PublicKey, Signature};
use base64::{Engine as _, engine::general_purpose::STANDARD};

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(transparent)]
pub struct PublicKeyDto(pub String);

impl From<PublicKey> for PublicKeyDto {
    fn from(pk: PublicKey) -> Self {
        Self(STANDARD.encode(pk.as_bytes()))
    }
}

impl TryFrom<PublicKeyDto> for PublicKey {
    type Error = String;
    fn try_from(dto: PublicKeyDto) -> Result<Self, Self::Error> {
        let bytes = STANDARD.decode(&dto.0).map_err(|e| e.to_string())?;
        PublicKey::try_from_bytes(&bytes)
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(transparent)]
pub struct SignatureDto(pub String);

impl From<Signature> for SignatureDto {
    fn from(sig: Signature) -> Self {
        Self(STANDARD.encode(sig.as_bytes()))
    }
}

impl TryFrom<SignatureDto> for Signature {
    type Error = String;
    fn try_from(dto: SignatureDto) -> Result<Self, Self::Error> {
        let bytes = STANDARD.decode(&dto.0).map_err(|e| e.to_string())?;
        Signature::try_from(bytes.as_slice())
    }
}
