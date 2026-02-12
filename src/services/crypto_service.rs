use crate::domain::crypto::{PublicKey, Signature};
use crate::error::{AppError, Result};
use ed25519_dalek::Verifier;
use xeddsa::ConvertMont;

#[derive(Clone, Debug, Default)]
pub struct CryptoService;

impl CryptoService {
    #[must_use]
    pub const fn new() -> Self {
        Self
    }

    /// Verifies an `XEdDSA` signature.
    ///
    /// `XEdDSA` is used to verify Ed25519 signatures against Curve25519 (Montgomery) public keys.
    /// Because a Montgomery X-coordinate corresponds to two Edwards points (sign bit 0 and 1),
    /// we try both to ensure compatibility with various client implementations and environments.
    ///
    /// # Errors
    /// Returns `AppError::BadRequest` if the signature is invalid.
    #[tracing::instrument(skip(self, public_key, message, signature), level = "debug")]
    #[allow(clippy::unused_self)]
    pub(crate) fn verify_signature(&self, public_key: &PublicKey, message: &[u8], signature: &Signature) -> Result<()> {
        let pk = xeddsa::xed25519::PublicKey(*public_key.as_crypto_bytes());

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

#[cfg(test)]
mod tests {
    use super::*;
    use crate::domain::crypto::DJB_KEY_PREFIX;
    use curve25519_dalek::edwards::CompressedEdwardsY;
    use rand::RngCore;
    use rand::rngs::OsRng;
    use xeddsa::xed25519::PrivateKey;
    use xeddsa::{CalculateKeyPair, Sign};

    #[test]
    fn test_verify_signature_exhaustive_robustness() {
        let service = CryptoService::new();
        let mut seed = [0u8; 32];
        OsRng.fill_bytes(&mut seed);
        let xed_priv = PrivateKey(seed);

        // We test both sign bits for the identity key creation
        for ik_sign in [0, 1] {
            // 1. Calculate public key with specific sign bit
            let (_, ed_pub) = xed_priv.calculate_key_pair(ik_sign);
            let mont_pub = CompressedEdwardsY(ed_pub).decompress().unwrap().to_montgomery().to_bytes();

            let mut ik_wire = [0u8; 33];
            ik_wire[0] = DJB_KEY_PREFIX;
            ik_wire[1..].copy_from_slice(&mont_pub);
            let ik_pub = PublicKey::new(ik_wire);

            // 2. Create both 32-byte and 33-byte messages
            let msg_32 = [0x42u8; 32];
            let mut msg_33 = [0x00u8; 33];
            msg_33[0] = DJB_KEY_PREFIX;
            msg_33[1..].copy_from_slice(&msg_32);

            // 3. Sign using XEdDSA (which uses the private key math)
            // Note: XEdDSA signing math is consistent with its verification math.
            let sig_32 = Signature::new(xed_priv.sign(&msg_32, OsRng));
            let sig_33 = Signature::new(xed_priv.sign(&msg_33, OsRng));

            assert!(
                service.verify_signature(&ik_pub, &msg_32, &sig_32).is_ok(),
                "Failed 32-byte msg for ik_sign={}",
                ik_sign
            );
            assert!(
                service.verify_signature(&ik_pub, &msg_33, &sig_33).is_ok(),
                "Failed 33-byte msg for ik_sign={}",
                ik_sign
            );
        }
    }
}
