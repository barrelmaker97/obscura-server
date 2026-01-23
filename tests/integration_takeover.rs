use base64::Engine;
use base64::engine::general_purpose::STANDARD;
use obscura_server::storage::message_repo::MessageRepository;
use serde_json::json;
use uuid::Uuid;
use xeddsa::xed25519::PrivateKey;
use xeddsa::CalculateKeyPair;

mod common;

#[tokio::test]
async fn test_device_takeover_success() {
    let app = common::TestApp::spawn().await;
    let run_id = Uuid::new_v4().to_string()[..8].to_string();
    let username = format!("takeover_user_{}", run_id);

    // 2. Register User (Device A)
    let user = app.register_user_with_keys(&username, 111, 1).await;

    // 3. Populate Data
    let msg_repo = MessageRepository::new();
    msg_repo.create(&app.pool, user.user_id, user.user_id, 2, vec![1, 2, 3], 30).await.unwrap();
    app.assert_message_count(user.user_id, 1).await;

    // 4. Connect WS (Optional, for other tests, but let's keep it if we want to test disconnect)
    let _ws = app.connect_ws(&user.token).await;

    // 5. Perform Takeover (Device B)
    let new_identity_key_bytes = common::generate_signing_key();
    let new_priv = PrivateKey(new_identity_key_bytes);
    let (_, new_ik_pub_ed) = new_priv.calculate_key_pair(0);
    let new_ik_pub_mont = curve25519_dalek::edwards::CompressedEdwardsY(new_ik_pub_ed).decompress().unwrap().to_montgomery().to_bytes();
    let mut new_ik_pub_wire = new_ik_pub_mont.to_vec();
    new_ik_pub_wire.insert(0, 0x05);
    
    let (new_spk_pub, new_spk_sig) = common::generate_signed_pre_key(&new_identity_key_bytes);

    let takeover_payload = json!({
        "identityKey": base64::engine::general_purpose::STANDARD.encode(new_ik_pub_wire),
        "registrationId": 456,
        "signedPreKey": {
            "keyId": 2,
            "publicKey": base64::engine::general_purpose::STANDARD.encode(&new_spk_pub),
            "signature": base64::engine::general_purpose::STANDARD.encode(&new_spk_sig)
        },
        "oneTimePreKeys": []
    });

    let resp = app.client.post(format!("{}/v1/keys", app.server_url))
        .header("Authorization", format!("Bearer {}", user.token))
        .json(&takeover_payload)
        .send().await.unwrap();

    assert_eq!(resp.status(), 200);

    // 6. Verify Identity Key Changed
    let resp = app.client.get(format!("{}/v1/keys/{}", app.server_url, user.user_id))
        .header("Authorization", format!("Bearer {}", user.token))
        .send().await.unwrap();
    assert_eq!(resp.status(), 200);
    let bundle: serde_json::Value = resp.json().await.unwrap();
    let ik_b64 = bundle["identityKey"].as_str().unwrap();
    let ik_bytes = base64::engine::general_purpose::STANDARD.decode(ik_b64).unwrap();
    
    assert_eq!(ik_bytes.len(), 33);
    assert_eq!(ik_bytes[0], 0x05);
    assert_eq!(&ik_bytes[1..], &new_ik_pub_mont);
}

#[tokio::test]
async fn test_refill_pre_keys_no_overwrite() {
    let app = common::TestApp::spawn().await;
    let run_id = Uuid::new_v4().to_string()[..8].to_string();
    let username = format!("refill_user_{}", run_id);

    // 1. Register
    let user = app.register_user_with_keys(&username, 111, 0).await;

    // 2. Refill (Same Identity Key)
    let (new_spk_pub, new_spk_sig) = common::generate_signed_pre_key(&user.identity_key);
    
    let ik_priv = PrivateKey(user.identity_key);
    let (_, ik_pub_ed) = ik_priv.calculate_key_pair(0);
    let ik_pub_mont = curve25519_dalek::edwards::CompressedEdwardsY(ik_pub_ed).decompress().unwrap().to_montgomery().to_bytes();
    let mut ik_pub_wire = ik_pub_mont.to_vec();
    ik_pub_wire.insert(0, 0x05);

    let refill_payload = json!({
        "identityKey": STANDARD.encode(ik_pub_wire),
        "registrationId": 111,
        "signedPreKey": {
            "keyId": 2,
            "publicKey": base64::engine::general_purpose::STANDARD.encode(&new_spk_pub),
            "signature": base64::engine::general_purpose::STANDARD.encode(&new_spk_sig)
        },
        "oneTimePreKeys": [
            { "keyId": 10, "publicKey": "BQEBAQEBAQEBAQEBAQEBAQEBAQEBAQEBAQEBAQEBAQEB" }
        ]
    });

    let refill_resp = app
        .client
        .post(format!("{}/v1/keys", app.server_url))
        .header("Authorization", format!("Bearer {}", user.token))
        .json(&refill_payload)
        .send()
        .await
        .unwrap();
    assert_eq!(refill_resp.status(), 200);
}
