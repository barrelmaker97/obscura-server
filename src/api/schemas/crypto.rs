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

#[cfg(test)]
mod tests {
    use super::*;
    use crate::domain::crypto::{DJB_KEY_PREFIX, PublicKey as DomainPublicKey, Signature as DomainSignature};

    #[test]
    fn test_public_key_schema_conversion() {
        let mut bytes = [0u8; 33];
        bytes[0] = DJB_KEY_PREFIX;
        let b64 = STANDARD.encode(bytes);
        
        let schema = PublicKey(b64);
        let domain: DomainPublicKey = schema.try_into().unwrap();
        assert_eq!(domain.as_bytes(), &bytes);
    }

    #[test]
    fn test_public_key_schema_malformed_base64() {
        let schema = PublicKey("!!!invalid!!!".to_string());
        let result: std::result::Result<DomainPublicKey, _> = schema.try_into();
        assert!(result.is_err());
    }

    #[test]
    fn test_public_key_schema_invalid_length() {
        let b64 = STANDARD.encode([0u8; 32]); // Missing prefix byte
        let schema = PublicKey(b64);
        let result: std::result::Result<DomainPublicKey, _> = schema.try_into();
        assert!(result.is_err());
    }

    #[test]
    fn test_signature_schema_conversion() {
        let bytes = [0u8; 64];
        let b64 = STANDARD.encode(bytes);
        
        let schema = Signature(b64);
        let domain: DomainSignature = schema.try_into().unwrap();
        assert_eq!(domain.as_bytes(), &bytes);
    }

    #[test]
    fn test_signature_schema_invalid_length() {
        let b64 = STANDARD.encode([0u8; 63]);
        let schema = Signature(b64);
        let result: std::result::Result<DomainSignature, _> = schema.try_into();
        assert!(result.is_err());
    }
}