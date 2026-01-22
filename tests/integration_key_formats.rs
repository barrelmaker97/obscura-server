use base64::{Engine as _, engine::general_purpose::STANDARD};
use curve25519_dalek::edwards::CompressedEdwardsY;
use ed25519_dalek::Signer;
use serde_json::json;
use uuid::Uuid;

mod common;

#[tokio::test]
async fn test_registration_with_x25519_identity_key() {
    // This test ensures we support clients that send X25519 (Montgomery) identity keys
    // but sign with the corresponding Ed25519 (Edwards) private key.
    // This is standard behavior for libsignal-javascript/typescript.

    // 1. Setup
    let app = common::TestApp::spawn().await;
    let run_id = Uuid::new_v4().to_string()[..8].to_string();
    let username = format!("signal_user_x25519_{}", run_id);

    // 2. Generate Keys (Ed25519)
    let identity_key = common::generate_signing_key();
    let ik_pub_ed = identity_key.verifying_key().to_bytes();

    // Convert Ed25519 Public Key (Edwards) -> X25519 Public Key (Montgomery)
    // We use the birational map equivalence.
    let ed_point = CompressedEdwardsY(ik_pub_ed).decompress().unwrap();
    let mont_point = ed_point.to_montgomery();
    let mut ik_pub_x25519 = mont_point.to_bytes().to_vec();

    // Standard Libsignal clients often prepend 0x05 to X25519 keys
    ik_pub_x25519.insert(0, 0x05);

    // Generate Signed PreKey (standard 32 bytes for the SPK itself)
    // The signature is created using the Ed25519 identity key!
    let (spk_pub, spk_sig) = common::generate_signed_pre_key(&identity_key);

    // 3. Construct Payload
    // WE SEND THE X25519 KEY, BUT THE SIGNATURE IS FROM THE ED25519 KEY
    let reg_payload = json!({
        "username": username,
        "password": "password",
        "registrationId": 123,
        "identityKey": STANDARD.encode(&ik_pub_x25519),
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
    let status = resp.status();
    let body = resp.text().await.unwrap();

    assert_eq!(status, 201, "Registration failed with X25519 identity key: {}", body);
}

#[tokio::test]
async fn test_registration_with_33_byte_signed_pre_key_strict() {
    // This test ensures we support clients that sign the full 33-byte key
    // (Explicit strictness)

    // 1. Setup
    let app = common::TestApp::spawn().await;
    let run_id = Uuid::new_v4().to_string()[..8].to_string();
    let username = format!("signal_user_strict_{}", run_id);

    // 2. Generate Keys
    let identity_key = common::generate_signing_key();
    let ik_pub = identity_key.verifying_key().to_bytes().to_vec();

    // Generate Signed PreKey (standard 32 bytes for the SPK itself)
    let (spk_pub_32, _spk_sig) = common::generate_signed_pre_key(&identity_key);

    // Create 33-byte SPK for upload
    let mut spk_pub_33 = spk_pub_32.clone();
    spk_pub_33.insert(0, 0x05);

    // Sign the 33-byte key (Strict client)
    let signature_over_33 = identity_key.sign(&spk_pub_33).to_bytes().to_vec();

    // 3. Construct Payload with 33-byte Signed Pre Key
    let reg_payload = json!({
        "username": username,
        "password": "password",
        "registrationId": 123,
        "identityKey": STANDARD.encode(&ik_pub),
        "signedPreKey": {
            "keyId": 1,
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

    assert_eq!(status, 201, "Registration failed with strict 33-byte signature: {}", body);
}

#[tokio::test]
async fn test_registration_with_libsignal_behavior() {
    // This test ensures we support Libsignal behavior:
    // Upload 33-byte key, but signature is over 32-byte key.

    // 1. Setup
    let app = common::TestApp::spawn().await;
    let run_id = Uuid::new_v4().to_string()[..8].to_string();
    let username = format!("signal_user_libsignal_{}", run_id);

    // 2. Generate Keys
    let identity_key = common::generate_signing_key();
    let ik_pub = identity_key.verifying_key().to_bytes().to_vec();

    // Generate Signed PreKey (standard 32 bytes for the SPK itself)
    let (spk_pub_32, spk_sig) = common::generate_signed_pre_key(&identity_key); // Signature over 32 bytes

    // Create 33-byte SPK for upload
    let mut spk_pub_33 = spk_pub_32.clone();
    spk_pub_33.insert(0, 0x05);

    // 3. Construct Payload with 33-byte Signed Pre Key but 32-byte signature
    let reg_payload = json!({
        "username": username,
        "password": "password",
        "registrationId": 123,
        "identityKey": STANDARD.encode(&ik_pub),
        "signedPreKey": {
            "keyId": 1,
            "publicKey": STANDARD.encode(&spk_pub_33), // Upload 33
            "signature": STANDARD.encode(&spk_sig)       // Signed 32
        },
        "oneTimePreKeys": []
    });

    // 4. Send Registration Request
    let resp = app.client.post(format!("{}/v1/users", app.server_url)).json(&reg_payload).send().await.unwrap();

    // 5. Assert Success
    let status = resp.status();
    let body = resp.text().await.unwrap();

    assert_eq!(status, 201, "Registration failed with Libsignal behavior (33-byte key, 32-byte sig): {}", body);
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

#[tokio::test]
async fn test_registration_with_high_bit_signature() {
    // This test ensures we support XEdDSA signatures where the high bit of 's' is set.
    // Standard Ed25519 libraries reject this, but we must handle it by clearing the bit.

    // 1. Setup
    let app = common::TestApp::spawn().await;
    let run_id = Uuid::new_v4().to_string()[..8].to_string();
    let username = format!("xeddsa_user_{}", run_id);

    // 2. Generate Keys
    let identity_key = common::generate_signing_key();
    let ik_pub_ed = identity_key.verifying_key().to_bytes();

    // Convert to X25519 (Montgomery) to trigger the correct server path
    let ed_point = CompressedEdwardsY(ik_pub_ed).decompress().unwrap();
    let mont_point = ed_point.to_montgomery();
    let mut ik_pub_x25519 = mont_point.to_bytes().to_vec();
    ik_pub_x25519.insert(0, 0x05);

    // Generate Signed PreKey
    let (spk_pub, spk_sig_canonical) = common::generate_signed_pre_key(&identity_key);

    // Force non-canonical by setting high bit
    let mut spk_sig_non_canonical = spk_sig_canonical;
    spk_sig_non_canonical[63] |= 0x80;

    // 3. Construct Payload
    let reg_payload = json!({
        "username": username,
        "password": "password",
        "registrationId": 123,
        "identityKey": STANDARD.encode(&ik_pub_x25519),
        "signedPreKey": {
            "keyId": 1,
            "publicKey": STANDARD.encode(&spk_pub),
            "signature": STANDARD.encode(&spk_sig_non_canonical)
        },
        "oneTimePreKeys": []
    });

    // 4. Send Registration Request
    let resp = app.client.post(format!("{}/v1/users", app.server_url)).json(&reg_payload).send().await.unwrap();

    // 5. Assert Success
    let status = resp.status();
    let body = resp.text().await.unwrap();

    assert_eq!(status, 201, "Registration failed with high-bit signature: {}", body);
}
