use serde::{Deserialize, Serialize};
use base64::{Engine as _, engine::general_purpose::STANDARD};

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(transparent)]
pub struct PublicKey(pub String);

impl From<crate::domain::crypto::PublicKey> for PublicKey {
    fn from(pk: crate::domain::crypto::PublicKey) -> Self {
        Self(STANDARD.encode(pk.as_bytes()))
    }
}

impl TryFrom<PublicKey> for crate::domain::crypto::PublicKey {
    type Error = String;
    fn try_from(schema: PublicKey) -> Result<Self, Self::Error> {
        let bytes = STANDARD.decode(&schema.0).map_err(|e| e.to_string())?;
        crate::domain::crypto::PublicKey::try_from_bytes(&bytes)
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(transparent)]
pub struct Signature(pub String);

impl From<crate::domain::crypto::Signature> for Signature {
    fn from(sig: crate::domain::crypto::Signature) -> Self {
        Self(STANDARD.encode(sig.as_bytes()))
    }
}

impl TryFrom<Signature> for crate::domain::crypto::Signature {
    type Error = String;
    fn try_from(schema: Signature) -> Result<Self, Self::Error> {
        let bytes = STANDARD.decode(&schema.0).map_err(|e| e.to_string())?;
        crate::domain::crypto::Signature::try_from(bytes.as_slice())
    }
}