use base64::{Engine as _, engine::general_purpose::STANDARD};
use serde::{Deserialize, Deserializer, Serialize, Serializer};

/// Strong type for public keys to distinguish between Edwards (Ed25519) and Montgomery (X25519) formats.
/// This handles parsing logic at the boundary.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum PublicKey {
    /// Standard 32-byte Ed25519 public key (or raw X25519 if no prefix provided)
    Edwards([u8; 32]),
    /// X25519 public key (derived from 33-byte 0x05-prefixed input)
    /// We store the inner 32 bytes.
    Montgomery([u8; 32]),
}

impl PublicKey {
    /// Returns the inner 32 bytes
    pub fn into_inner(self) -> [u8; 32] {
        match self {
            PublicKey::Edwards(b) => b,
            PublicKey::Montgomery(b) => b,
        }
    }

    pub fn as_bytes(&self) -> &[u8; 32] {
        match self {
            PublicKey::Edwards(b) => b,
            PublicKey::Montgomery(b) => b,
        }
    }

    /// Returns the bytes as they should be stored or transmitted (wire format).
    /// Edwards -> 32 bytes
    /// Montgomery -> 33 bytes (0x05 prefix)
    pub fn to_wire_bytes(&self) -> Vec<u8> {
        match self {
            PublicKey::Edwards(b) => b.to_vec(),
            PublicKey::Montgomery(b) => {
                let mut v = Vec::with_capacity(33);
                v.push(0x05);
                v.extend_from_slice(b);
                v
            }
        }
    }

    /// Tries to create a PublicKey from wire bytes (32 or 33 bytes).
    pub fn try_from_bytes(bytes: &[u8]) -> Result<Self, String> {
        match bytes.len() {
            32 => {
                let mut arr = [0u8; 32];
                arr.copy_from_slice(bytes);
                Ok(PublicKey::Edwards(arr))
            }
            33 => {
                if bytes[0] != 0x05 {
                    return Err("Invalid key prefix for 33-byte key (expected 0x05)".into());
                }
                let mut arr = [0u8; 32];
                arr.copy_from_slice(&bytes[1..]);
                Ok(PublicKey::Montgomery(arr))
            }
            _ => Err(format!("Invalid key length: {}", bytes.len()))
        }
    }
}

impl TryFrom<&[u8]> for PublicKey {
    type Error = String;
    fn try_from(bytes: &[u8]) -> Result<Self, Self::Error> {
        PublicKey::try_from_bytes(bytes)
    }
}

impl TryFrom<Vec<u8>> for PublicKey {
    type Error = String;
    fn try_from(bytes: Vec<u8>) -> Result<Self, Self::Error> {
        PublicKey::try_from_bytes(&bytes)
    }
}

impl From<PublicKey> for Vec<u8> {
    fn from(key: PublicKey) -> Self {
        key.to_wire_bytes()
    }
}

impl Serialize for PublicKey {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: Serializer,
    {
        let bytes = self.to_wire_bytes();
        let b64 = STANDARD.encode(bytes);
        serializer.serialize_str(&b64)
    }
}

impl<'de> Deserialize<'de> for PublicKey {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        let s = String::deserialize(deserializer)?;
        let bytes = STANDARD.decode(&s).map_err(serde::de::Error::custom)?;

        PublicKey::try_from_bytes(&bytes).map_err(serde::de::Error::custom)
    }
}

/// Strong type for 64-byte Ed25519 signatures.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Signature([u8; 64]);

impl Signature {
    pub fn as_bytes(&self) -> &[u8; 64] {
        &self.0
    }

    pub fn to_vec(&self) -> Vec<u8> {
        self.0.to_vec()
    }
}

impl TryFrom<&[u8]> for Signature {
    type Error = String;
    fn try_from(bytes: &[u8]) -> Result<Self, Self::Error> {
        if bytes.len() == 64 {
            let mut arr = [0u8; 64];
            arr.copy_from_slice(bytes);
            Ok(Signature(arr))
        } else {
            Err(format!("Invalid signature length: {} (expected 64)", bytes.len()))
        }
    }
}

impl TryFrom<Vec<u8>> for Signature {
    type Error = String;
    fn try_from(bytes: Vec<u8>) -> Result<Self, Self::Error> {
        Signature::try_from(bytes.as_slice())
    }
}

impl From<Signature> for Vec<u8> {
    fn from(sig: Signature) -> Self {
        sig.to_vec()
    }
}

impl Serialize for Signature {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: Serializer,
    {
        let b64 = STANDARD.encode(self.0);
        serializer.serialize_str(&b64)
    }
}

impl<'de> Deserialize<'de> for Signature {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        let s = String::deserialize(deserializer)?;
        let bytes = STANDARD.decode(&s).map_err(serde::de::Error::custom)?;

        Signature::try_from(bytes.as_slice()).map_err(serde::de::Error::custom)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_deserialize_edwards_32() {
        let bytes = [1u8; 32];
        let b64 = STANDARD.encode(bytes);
        let key: PublicKey = serde_json::from_str(&format!("\"{}\"", b64)).unwrap();
        assert_eq!(key, PublicKey::Edwards(bytes));
    }

    #[test]
    fn test_deserialize_montgomery_33() {
        let mut bytes = [2u8; 33];
        bytes[0] = 0x05; // Prefix
        let inner = [2u8; 32]; // Rest
        bytes[1..].copy_from_slice(&inner);

        let b64 = STANDARD.encode(bytes);
        let key: PublicKey = serde_json::from_str(&format!("\"{}\"", b64)).unwrap();
        assert_eq!(key, PublicKey::Montgomery(inner));
    }

    #[test]
    fn test_deserialize_invalid_len() {
        let bytes = [0u8; 31];
        let b64 = STANDARD.encode(bytes);
        let res: Result<PublicKey, _> = serde_json::from_str(&format!("\"{}\"", b64));
        assert!(res.is_err());
    }

    #[test]
    fn test_serialize_roundtrip() {
        let key = PublicKey::Montgomery([7u8; 32]);
        let json = serde_json::to_string(&key).unwrap();
        let back: PublicKey = serde_json::from_str(&json).unwrap();
        assert_eq!(key, back);
    }

    #[test]
    fn test_signature_roundtrip() {
        let bytes = [9u8; 64];
        let sig = Signature::try_from(&bytes[..]).unwrap();
        let json = serde_json::to_string(&sig).unwrap();
        let back: Signature = serde_json::from_str(&json).unwrap();
        assert_eq!(sig, back);
    }
}