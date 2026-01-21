use base64::{Engine as _, engine::general_purpose::STANDARD};
use ed25519_dalek::Signer;
use serde_json::json;
use uuid::Uuid;

mod common;

#[tokio::test]
async fn test_registration_with_33_byte_signed_pre_key() {
    // This test ensures the KeyService correctly handles the 33-byte SignedPreKey
    // by stripping the prefix before verification.

    // 1. Setup
    let app = common::TestApp::spawn().await;
    let run_id = Uuid::new_v4().to_string()[..8].to_string();
    let username = format!("signal_user_spk_{}", run_id);

    // 2. Generate Keys
    let identity_key = common::generate_signing_key();
    let ik_pub = identity_key.verifying_key().to_bytes().to_vec();

    // Generate Signed PreKey (standard 32 bytes for the SPK itself)
    let (spk_pub_32, spk_sig) = common::generate_signed_pre_key(&identity_key);

    // Create 33-byte SPK for upload
    let mut spk_pub_33 = spk_pub_32.clone();
    spk_pub_33.insert(0, 0x05);

    // Sign the 33-byte key (Real world behavior)
    let signature_over_33 = identity_key.sign(&spk_pub_33).to_bytes().to_vec();

    // 3. Construct Payload with 33-byte Signed Pre Key
    let reg_payload = json!({
        "username": username,
        "password": "password",
        "registrationId": 123,
        "identityKey": STANDARD.encode(&ik_pub),
        "signedPreKey": {
            "keyId": 1,
            // The server receives 33 bytes here
            "publicKey": STANDARD.encode(&spk_pub_33),
            "signature": STANDARD.encode(&signature_over_33)
        },
        "oneTimePreKeys": []
    });

    // 4. Send Registration Request
    let resp = app.client.post(format!("{}/v1/users", app.server_url)).json(&reg_payload).send().await.unwrap();

    // 5. Assert Success
    let status = resp.status();
    let body = resp.text().await.unwrap();

    assert_eq!(status, 201, "Registration failed with 33-byte Signed Pre Key: {}", body);
}

#[tokio::test]
async fn test_registration_with_33_byte_identity_key() {
    // 1. Setup
    let app = common::TestApp::spawn().await;
    let run_id = Uuid::new_v4().to_string()[..8].to_string();
    let username = format!("signal_user_{}", run_id);

    // 2. Generate Keys
    // Generate standard 32-byte keys first
    let identity_key = common::generate_signing_key();
    let mut ik_pub_33 = identity_key.verifying_key().to_bytes().to_vec();

    // Prepend 0x05 to simulate Libsignal format (Curve25519/Ed25519 marker)
    ik_pub_33.insert(0, 0x05);
    assert_eq!(ik_pub_33.len(), 33);

    // Generate Signed PreKey (standard 32 bytes for the SPK itself)
    let (spk_pub, spk_sig) = common::generate_signed_pre_key(&identity_key);

    // 3. Construct Payload with 33-byte Identity Key
    let reg_payload = json!({
        "username": username,
        "password": "password",
        "registrationId": 123,
        // The server receives 33 bytes here
        "identityKey": STANDARD.encode(&ik_pub_33),
        "signedPreKey": {
            "keyId": 1,
            "publicKey": STANDARD.encode(&spk_pub),
            "signature": STANDARD.encode(&spk_sig)
        },
        "oneTimePreKeys": []
    });

    // 4. Send Registration Request
    let resp = app.client.post(format!("{}/v1/users", app.server_url)).json(&reg_payload).send().await.unwrap();

    // 5. Assert Success
    // If the server didn't handle the extra byte, this would be 400 Bad Request
    let status = resp.status();
    let body = resp.text().await.unwrap();

    assert_eq!(status, 201, "Registration failed with 33-byte key: {}", body);
}
