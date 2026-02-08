use crate::error::{AppError, Result};
use ed25519_dalek::Verifier;

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
            return Err(format!("Invalid key prefix (expected 0x{:02x})", DJB_KEY_PREFIX));
        }
        let mut arr = [0u8; 33];
        arr.copy_from_slice(bytes);
        Ok(PublicKey(arr))
    }

    /// Verifies an XEdDSA signature.
    ///
    /// XEdDSA is used to verify Ed25519 signatures against Curve25519 (Montgomery) public keys.
    /// Because a Montgomery X-coordinate corresponds to two Edwards points (sign bit 0 and 1),
    /// we try both to ensure compatibility with various client implementations and environments.
    #[tracing::instrument(skip(self, message, signature), level = "debug")]
    pub fn verify(&self, message: &[u8], signature: &Signature) -> Result<()> {
        let pk = xeddsa::xed25519::PublicKey(*self.as_crypto_bytes());

        use xeddsa::ConvertMont;
        let sig_bytes = signature.as_bytes();

        // We must clear the 255th bit of 's' for standard Ed25519 libraries.
        // XEdDSA uses this bit to represent the sign of the recovered point.
        let mut sig_canonical = *sig_bytes;
        sig_canonical[63] &= 0x7F;
        let sig_obj = ed25519_dalek::Signature::from_bytes(&sig_canonical);

        // Try both possible Edwards points for the Montgomery public key.
        // Some client environments (like JS polyfills) may choose the alternative point.
        for sign_bit in [0, 1] {
            if let Ok(ed_pk_bytes) = pk.convert_mont(sign_bit)
                && let Ok(ed_pk) = ed25519_dalek::VerifyingKey::from_bytes(&ed_pk_bytes)
                && ed_pk.verify(message, &sig_obj).is_ok()
            {
                return Ok(());
            }
        }

        Err(AppError::BadRequest("Invalid signature".into()))
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
    use curve25519_dalek::edwards::CompressedEdwardsY;
    use xeddsa::xed25519::PrivateKey;
    use xeddsa::{CalculateKeyPair, Sign};
    use rand::RngCore;
    use rand::rngs::OsRng;

    #[test]
    fn test_as_crypto_bytes() {
        let mut bytes = [2u8; 33];
        bytes[0] = DJB_KEY_PREFIX;
        let inner = [3u8; 32];
        bytes[1..].copy_from_slice(&inner);

        let key = PublicKey::try_from_bytes(&bytes).unwrap();
        assert_eq!(key.as_crypto_bytes(), &inner);
    }

    #[test]
    fn test_verify_signature_exhaustive_robustness() {
        let mut seed = [0u8; 32];
        OsRng.fill_bytes(&mut seed);
        let xed_priv = PrivateKey(seed);

        // We test both sign bits for the identity key creation
        for ik_sign in [0, 1] {
            // 1. Calculate public key with specific sign bit
            let (_, ed_pub) = xed_priv.calculate_key_pair(ik_sign);
            let mont_pub = CompressedEdwardsY(ed_pub).decompress().unwrap().to_montgomery().to_bytes();

            let mut ik_wire = [0u8; 33];
            ik_wire[0] = 0x05;
            ik_wire[1..].copy_from_slice(&mont_pub);
            let ik_pub = PublicKey::new(ik_wire);

            // 2. Create both 32-byte and 33-byte messages
            let msg_32 = [0x42u8; 32];
            let mut msg_33 = [0x00u8; 33];
            msg_33[0] = 0x05;
            msg_33[1..].copy_from_slice(&msg_32);

            // 3. Sign using XEdDSA (which uses the private key math)
            // Note: XEdDSA signing math is consistent with its verification math.
            let sig_32 = Signature::new(xed_priv.sign(&msg_32, OsRng));
            let sig_33 = Signature::new(xed_priv.sign(&msg_33, OsRng));

            assert!(ik_pub.verify(&msg_32, &sig_32).is_ok(), "Failed 32-byte msg for ik_sign={}", ik_sign);
            assert!(ik_pub.verify(&msg_33, &sig_33).is_ok(), "Failed 33-byte msg for ik_sign={}", ik_sign);
        }
    }
}
