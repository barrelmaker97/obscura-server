/// Prefix byte used by Signal/DJB for Montgomery (X25519) keys.
pub const DJB_KEY_PREFIX: u8 = 0x05;

/// Strong type for public keys.
/// We store the full 33-byte wire format (DJB_KEY_PREFIX + 32-byte Montgomery key).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PublicKey([u8; 33]);

impl PublicKey {
    pub fn new(bytes: [u8; 33]) -> Self {
        Self(bytes)
    }

    /// Returns the inner 32 bytes for cryptographic operations.
    pub fn as_crypto_bytes(&self) -> &[u8; 32] {
        self.0[1..].try_into().expect("PublicKey must be 33 bytes")
    }

    pub fn as_bytes(&self) -> &[u8; 33] {
        &self.0
    }

    /// Tries to create a PublicKey from wire bytes (MUST be 33 bytes with 0x05 prefix).
    pub fn try_from_bytes(bytes: &[u8]) -> std::result::Result<Self, String> {
        if bytes.len() != 33 {
            return Err(format!("Invalid key length: {} (expected 33 bytes with 0x05 prefix)", bytes.len()));
        }
        if bytes[0] != DJB_KEY_PREFIX {
            return Err(format!("Invalid key prefix (expected 0x{DJB_KEY_PREFIX:02x})"));
        }
        let mut arr = [0u8; 33];
        arr.copy_from_slice(bytes);
        Ok(PublicKey(arr))
    }
}

impl TryFrom<&[u8]> for PublicKey {
    type Error = String;
    fn try_from(bytes: &[u8]) -> std::result::Result<Self, Self::Error> {
        PublicKey::try_from_bytes(bytes)
    }
}

impl TryFrom<Vec<u8>> for PublicKey {
    type Error = String;
    fn try_from(bytes: Vec<u8>) -> std::result::Result<Self, Self::Error> {
        PublicKey::try_from_bytes(&bytes)
    }
}

impl From<PublicKey> for Vec<u8> {
    fn from(key: PublicKey) -> Self {
        key.0.to_vec()
    }
}

/// Strong type for 64-byte Ed25519 signatures.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Signature([u8; 64]);

impl Signature {
    pub fn new(bytes: [u8; 64]) -> Self {
        Self(bytes)
    }

    pub fn as_bytes(&self) -> &[u8; 64] {
        &self.0
    }
}

impl TryFrom<&[u8]> for Signature {
    type Error = String;
    fn try_from(bytes: &[u8]) -> std::result::Result<Self, Self::Error> {
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
    fn try_from(bytes: Vec<u8>) -> std::result::Result<Self, Self::Error> {
        Signature::try_from(bytes.as_slice())
    }
}

impl From<Signature> for Vec<u8> {
    fn from(sig: Signature) -> Self {
        sig.0.to_vec()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_as_crypto_bytes() {
        let mut bytes = [2u8; 33];
        bytes[0] = DJB_KEY_PREFIX;
        let inner = [3u8; 32];
        bytes[1..].copy_from_slice(&inner);

        let key = PublicKey::try_from_bytes(&bytes).unwrap();
        assert_eq!(key.as_crypto_bytes(), &inner);
    }
}
