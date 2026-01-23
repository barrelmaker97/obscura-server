use base64::{Engine as _, engine::general_purpose::STANDARD};
use curve25519_dalek::edwards::CompressedEdwardsY;
use serde_json::json;
use uuid::Uuid;
use xeddsa::xed25519::PrivateKey;
use xeddsa::{CalculateKeyPair, Sign};

mod common;

/// Helper to generate a Montgomery key pair and its wire format (33 bytes)
fn generate_test_keys() -> (PrivateKey, [u8; 32], Vec<u8>) {
    let ik_bytes = common::generate_signing_key();
    let priv_key = PrivateKey(ik_bytes);
    let (_, ik_pub_ed) = priv_key.calculate_key_pair(0);
    let ik_pub_mont = CompressedEdwardsY(ik_pub_ed).decompress().unwrap().to_montgomery().to_bytes();
    let mut ik_pub_wire = ik_pub_mont.to_vec();
    ik_pub_wire.insert(0, 0x05);
    (priv_key, ik_pub_mont, ik_pub_wire)
}

#[tokio::test]
async fn test_registration_matches_typescript_client_behavior() {
    // This test EXACTLY matches the @privacyresearch/libsignal-protocol-typescript implementation:
    // 1. Transmit Identity Key as 33 bytes (0x05 prefix)
    // 2. Transmit SignedPreKey as 33 bytes (0x05 prefix)
    // 3. Calculate Signature over the 33-byte WIRE format of the SignedPreKey
    
    let app = common::TestApp::spawn().await;
    let username = format!("user_ts_client_{}", &Uuid::new_v4().to_string()[..8]);

    let (ik_priv, _ik_pub_32, ik_pub_wire) = generate_test_keys();
    let (_spk_priv, _spk_pub_inner, spk_pub_wire) = generate_test_keys();

    // Sign the 33-byte wire format (This is what libsignal-protocol-typescript does)
    let signature: [u8; 64] = ik_priv.sign(&spk_pub_wire, rand::rngs::OsRng);

    let reg_payload = json!({
        "username": username,
        "password": "password",
        "registrationId": 123,
        "identityKey": STANDARD.encode(&ik_pub_wire),
        "signedPreKey": {
            "keyId": 1,
            "publicKey": STANDARD.encode(&spk_pub_wire),
            "signature": STANDARD.encode(signature)
        },
        "oneTimePreKeys": []
    });

    let resp = app.client.post(format!("{}/v1/users", app.server_url)).json(&reg_payload).send().await.unwrap();
    assert_eq!(resp.status(), 201, "Server rejected the format used by the TypeScript client: {}", resp.text().await.unwrap());
}
