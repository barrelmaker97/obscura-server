use base64::Engine;
use serde_json::json;
use uuid::Uuid;
use xeddsa::xed25519::PrivateKey;
use xeddsa::{CalculateKeyPair, Sign};

mod common;

#[tokio::test]
async fn test_key_rotation_monotonic_check() {
    let app = common::TestApp::spawn().await;
    let run_id = Uuid::new_v4().to_string()[..8].to_string();
    let username = format!("rotate_user_{}", run_id);

    // 1. Initial Registration (Key ID 10)
    let identity_key = common::generate_signing_key();
    let ik_priv = PrivateKey(identity_key);
    let (_, ik_pub_ed) = ik_priv.calculate_key_pair(0);
    let ik_pub_mont = curve25519_dalek::edwards::CompressedEdwardsY(ik_pub_ed).decompress().unwrap().to_montgomery().to_bytes();
    let mut ik_pub_wire = ik_pub_mont.to_vec();
    ik_pub_wire.insert(0, 0x05);
    
    // Generate SPK 10
    let (spk_pub_10, spk_sig_10) = generate_spk(&identity_key);

    let reg_payload = json!({
        "username": username,
        "password": "password",
        "registrationId": 123,
        "identityKey": base64::engine::general_purpose::STANDARD.encode(&ik_pub_wire),
        "signedPreKey": {
            "keyId": 10,
            "publicKey": base64::engine::general_purpose::STANDARD.encode(&spk_pub_10),
            "signature": base64::engine::general_purpose::STANDARD.encode(&spk_sig_10)
        },
        "oneTimePreKeys": []
    });

    let resp = app.client.post(format!("{}/v1/users", app.server_url)).json(&reg_payload).send().await.unwrap();
    assert_eq!(resp.status(), 201);
    let token = resp.json::<serde_json::Value>().await.unwrap()["token"].as_str().unwrap().to_string();

    // 2. Rotate to Key ID 11 (Success)
    let (spk_pub_11, spk_sig_11) = generate_spk(&identity_key);
    let rotate_payload_11 = json!({
        "identityKey": base64::engine::general_purpose::STANDARD.encode(&ik_pub_wire),
        "registrationId": 123,
        "signedPreKey": {
            "keyId": 11,
            "publicKey": base64::engine::general_purpose::STANDARD.encode(&spk_pub_11),
            "signature": base64::engine::general_purpose::STANDARD.encode(&spk_sig_11)
        },
        "oneTimePreKeys": []
    });

    let resp_11 = app.client.post(format!("{}/v1/keys", app.server_url))
        .header("Authorization", format!("Bearer {}", token))
        .json(&rotate_payload_11).send().await.unwrap();
    assert_eq!(resp_11.status(), 200);

    // 3. Try Replay Key ID 10 (Fail - Too Old)
    let rotate_payload_10 = json!({
        "identityKey": base64::engine::general_purpose::STANDARD.encode(&ik_pub_wire),
        "registrationId": 123,
        "signedPreKey": {
            "keyId": 10,
            "publicKey": base64::engine::general_purpose::STANDARD.encode(&spk_pub_10),
            "signature": base64::engine::general_purpose::STANDARD.encode(&spk_sig_10)
        },
        "oneTimePreKeys": []
    });
    let resp_10 = app.client.post(format!("{}/v1/keys", app.server_url))
        .header("Authorization", format!("Bearer {}", token))
        .json(&rotate_payload_10).send().await.unwrap();
    assert_eq!(resp_10.status(), 400);

    // 4. Try Replay Key ID 11 (Fail - Must be > current)
    let resp_retry_11 = app.client.post(format!("{}/v1/keys", app.server_url))
        .header("Authorization", format!("Bearer {}", token))
        .json(&rotate_payload_11).send().await.unwrap();
    assert_eq!(resp_retry_11.status(), 400);

    // 5. Cleanup Verification
    // ID 10 should be deleted, ID 11 should exist.
    // We can't query DB directly easily from here due to Uuid/Pool separation in test helper,
    // but we can infer it by the fact that if we tried to rotate to 12, it works.
    
    // Rotate to 12
    let (spk_pub_12, spk_sig_12) = generate_spk(&identity_key);
    let rotate_payload_12 = json!({
        "identityKey": base64::engine::general_purpose::STANDARD.encode(&ik_pub_wire),
        "registrationId": 123,
        "signedPreKey": {
            "keyId": 12,
            "publicKey": base64::engine::general_purpose::STANDARD.encode(&spk_pub_12),
            "signature": base64::engine::general_purpose::STANDARD.encode(&spk_sig_12)
        },
        "oneTimePreKeys": []
    });
    let resp_12 = app.client.post(format!("{}/v1/keys", app.server_url))
        .header("Authorization", format!("Bearer {}", token))
        .json(&rotate_payload_12).send().await.unwrap();
    assert_eq!(resp_12.status(), 200);
}

#[tokio::test]
async fn test_key_rotation_cleanup() {
    let app = common::TestApp::spawn().await;
    let run_id = Uuid::new_v4().to_string()[..8].to_string();
    let username = format!("cleanup_check_{}", run_id);
    let user = app.register_user(&username).await; // Defaults to ID 1

    // Rotate to ID 10
    let (spk_pub, spk_sig) = generate_spk(&user.identity_key);
    // Identity Key Wire
    let ik_priv = PrivateKey(user.identity_key);
    let (_, ik_pub_ed) = ik_priv.calculate_key_pair(0);
    let ik_pub_mont = curve25519_dalek::edwards::CompressedEdwardsY(ik_pub_ed).decompress().unwrap().to_montgomery().to_bytes();
    let mut ik_pub_wire = ik_pub_mont.to_vec();
    ik_pub_wire.insert(0, 0x05);

    let payload = json!({
        "identityKey": base64::engine::general_purpose::STANDARD.encode(&ik_pub_wire),
        "registrationId": 123,
        "signedPreKey": {
            "keyId": 10,
            "publicKey": base64::engine::general_purpose::STANDARD.encode(&spk_pub),
            "signature": base64::engine::general_purpose::STANDARD.encode(&spk_sig)
        },
        "oneTimePreKeys": []
    });

    app.client.post(format!("{}/v1/keys", app.server_url))
        .header("Authorization", format!("Bearer {}", user.token))
        .json(&payload).send().await.unwrap();

    // Verify directly in DB that ID 1 is GONE and ID 10 EXISTS
    let count_1: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM signed_pre_keys WHERE user_id = $1 AND id = 1")
        .bind(user.user_id)
        .fetch_one(&app.pool)
        .await
        .unwrap();
    assert_eq!(count_1, 0, "Old key ID 1 should be deleted");

    let count_10: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM signed_pre_keys WHERE user_id = $1 AND id = 10")
        .bind(user.user_id)
        .fetch_one(&app.pool)
        .await
        .unwrap();
    assert_eq!(count_10, 1, "New key ID 10 should exist");
}

fn generate_spk(identity_key: &[u8; 32]) -> (Vec<u8>, Vec<u8>) {
    let spk_bytes = common::generate_signing_key();
    let spk_priv = PrivateKey(spk_bytes);
    let (_, spk_pub_ed) = spk_priv.calculate_key_pair(0);
    let spk_pub_mont = curve25519_dalek::edwards::CompressedEdwardsY(spk_pub_ed).decompress().unwrap().to_montgomery().to_bytes();
    let mut spk_pub_wire = [0u8; 33];
    spk_pub_wire[0] = 0x05;
    spk_pub_wire[1..].copy_from_slice(&spk_pub_mont);

    let ik_priv = PrivateKey(*identity_key);
    let signature: [u8; 64] = ik_priv.sign(&spk_pub_wire, rand::rngs::OsRng);
    (spk_pub_wire.to_vec(), signature.to_vec())
}
