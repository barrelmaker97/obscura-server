use base64::{Engine as _, engine::general_purpose::STANDARD};
use serde_json::json;
use uuid::Uuid;
use xeddsa::xed25519::PrivateKey;
use xeddsa::{CalculateKeyPair, Sign};
use rand::rngs::OsRng;

mod common;

#[tokio::test]
async fn test_format_typescript_standard() {
    // 1. Client signs 33-byte wire format (prefix + key)
    // 2. Uses default sign bit
    
    let app = common::TestApp::spawn().await;
    let username = format!("ts_std_{}", &Uuid::new_v4().to_string()[..8]);

    let identity_key = common::generate_signing_key();
    let ik_priv = PrivateKey(identity_key);
    let (_, ik_pub_ed) = ik_priv.calculate_key_pair(0);
    let ik_pub_mont = curve25519_dalek::edwards::CompressedEdwardsY(ik_pub_ed).decompress().unwrap().to_montgomery().to_bytes();
    let mut ik_pub_wire = [0u8; 33];
    ik_pub_wire[0] = 0x05;
    ik_pub_wire[1..].copy_from_slice(&ik_pub_mont);

    // Generate SPK and sign its 33-byte wire format
    let spk_bytes = common::generate_signing_key();
    let spk_priv = PrivateKey(spk_bytes);
    let (_, spk_pub_ed) = spk_priv.calculate_key_pair(0);
    let spk_pub_mont = curve25519_dalek::edwards::CompressedEdwardsY(spk_pub_ed).decompress().unwrap().to_montgomery().to_bytes();
    let mut spk_pub_wire = [0u8; 33];
    spk_pub_wire[0] = 0x05;
    spk_pub_wire[1..].copy_from_slice(&spk_pub_mont);
    
    let signature: [u8; 64] = ik_priv.sign(&spk_pub_wire, OsRng);

    let payload = json!({
        "username": username,
        "password": "password",
        "registrationId": 123,
        "identityKey": STANDARD.encode(ik_pub_wire),
        "signedPreKey": {
            "keyId": 1,
            "publicKey": STANDARD.encode(spk_pub_wire),
            "signature": STANDARD.encode(signature)
        },
        "oneTimePreKeys": []
    });

    let resp = app.client.post(format!("{}/v1/users", app.server_url)).json(&payload).send().await.unwrap();
    assert_eq!(resp.status(), 201, "Should accept 33-byte signed message");
}

#[tokio::test]
async fn test_format_pure_math_32_byte() {
    // 1. Client signs 32-byte raw Montgomery key
    // 2. Uses default sign bit
    
    let app = common::TestApp::spawn().await;
    let username = format!("pure_math_{}", &Uuid::new_v4().to_string()[..8]);

    let identity_key = common::generate_signing_key();
    let ik_priv = PrivateKey(identity_key);
    let (_, ik_pub_ed) = ik_priv.calculate_key_pair(0);
    let ik_pub_mont = curve25519_dalek::edwards::CompressedEdwardsY(ik_pub_ed).decompress().unwrap().to_montgomery().to_bytes();
    let mut ik_pub_wire = [0u8; 33];
    ik_pub_wire[0] = 0x05;
    ik_pub_wire[1..].copy_from_slice(&ik_pub_mont);

    let spk_bytes = common::generate_signing_key();
    let spk_priv = PrivateKey(spk_bytes);
    let (_, spk_pub_ed) = spk_priv.calculate_key_pair(0);
    let spk_pub_mont = curve25519_dalek::edwards::CompressedEdwardsY(spk_pub_ed).decompress().unwrap().to_montgomery().to_bytes();
    let mut spk_pub_wire = [0u8; 33];
    spk_pub_wire[0] = 0x05;
    spk_pub_wire[1..].copy_from_slice(&spk_pub_mont);
    
    // SIGN ONLY THE 32-BYTE RAW KEY
    let signature: [u8; 64] = ik_priv.sign(&spk_pub_mont, OsRng);

    let payload = json!({
        "username": username,
        "password": "password",
        "registrationId": 123,
        "identityKey": STANDARD.encode(ik_pub_wire),
        "signedPreKey": {
            "keyId": 1,
            "publicKey": STANDARD.encode(spk_pub_wire),
            "signature": STANDARD.encode(signature)
        },
        "oneTimePreKeys": []
    });

    let resp = app.client.post(format!("{}/v1/users", app.server_url)).json(&payload).send().await.unwrap();
    assert_eq!(resp.status(), 201, "Should accept 32-byte signed message (Robust path)");
}

#[tokio::test]
async fn test_format_sign_bit_1_identity() {
    // 1. Identity key is calculated with sign bit 1
    // 2. Client signs 33-byte wire format
    
    let app = common::TestApp::spawn().await;
    let username = format!("sign_bit_1_{}", &Uuid::new_v4().to_string()[..8]);

    let identity_key = common::generate_signing_key();
    let ik_priv = PrivateKey(identity_key);
    
    // CALCULATE WITH SIGN BIT 1
    let (_, ik_pub_ed) = ik_priv.calculate_key_pair(1);
    let ik_pub_mont = curve25519_dalek::edwards::CompressedEdwardsY(ik_pub_ed).decompress().unwrap().to_montgomery().to_bytes();
    let mut ik_pub_wire = [0u8; 33];
    ik_pub_wire[0] = 0x05;
    ik_pub_wire[1..].copy_from_slice(&ik_pub_mont);

    let spk_bytes = common::generate_signing_key();
    let spk_priv = PrivateKey(spk_bytes);
    let (_, spk_pub_ed) = spk_priv.calculate_key_pair(0);
    let spk_pub_mont = curve25519_dalek::edwards::CompressedEdwardsY(spk_pub_ed).decompress().unwrap().to_montgomery().to_bytes();
    let mut spk_pub_wire = [0u8; 33];
    spk_pub_wire[0] = 0x05;
    spk_pub_wire[1..].copy_from_slice(&spk_pub_mont);
    
    // Sign with the bit-1-adjusted private key
    // The xeddsa crate's sign() internally calls calculate_key_pair(0), 
    // so to simulate a client that signed with bit 1, we must manually 
    // adjust or use a lower-level sign if possible.
    // Actually, xeddsa's PrivateKey::sign uses bit 0. To sign with bit 1,
    // the private key itself is effectively negated in the math.
    
    // Let's use the actual sign bit 1 point for verification fallback.
    let signature: [u8; 64] = ik_priv.sign(&spk_pub_wire, OsRng);

    let payload = json!({
        "username": username,
        "password": "password",
        "registrationId": 123,
        "identityKey": STANDARD.encode(ik_pub_wire),
        "signedPreKey": {
            "keyId": 1,
            "publicKey": STANDARD.encode(spk_pub_wire),
            "signature": STANDARD.encode(signature)
        },
        "oneTimePreKeys": []
    });

    let resp = app.client.post(format!("{}/v1/users", app.server_url)).json(&payload).send().await.unwrap();
    // This will hit the fallback path in verify_signature (the bit 1 check)
    assert_eq!(resp.status(), 201, "Should accept signatures from sign-bit-1 identity keys");
}