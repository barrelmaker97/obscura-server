use base64::Engine;
use obscura_server::storage::key_repo::KeyRepository;
use obscura_server::storage::message_repo::MessageRepository;
use serde_json::json;
use uuid::Uuid;

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

    // 4. Connect WebSocket (Device A)
    let mut ws = app.connect_ws(&user.token).await;

    // 5. Perform Takeover (Device B)
    let new_identity_key = common::generate_signing_key();
    let new_ik_pub = new_identity_key.verifying_key().to_bytes();
    let (new_spk_pub, new_spk_sig) = common::generate_signed_pre_key(&new_identity_key);

    let takeover_payload = json!({
        "identityKey": base64::engine::general_purpose::STANDARD.encode(new_ik_pub),
        "registrationId": 222,
        "signedPreKey": {
            "keyId": 2,
            "publicKey": base64::engine::general_purpose::STANDARD.encode(&new_spk_pub),
            "signature": base64::engine::general_purpose::STANDARD.encode(&new_spk_sig)
        },
        "oneTimePreKeys": [
            { "keyId": 10, "publicKey": "QkJCQkJCQkJCQkJCQkJCQkJCQkJCQkJCQkJCQkJCQkI=" }
        ]
    });

    let takeover_resp = app
        .client
        .post(format!("{}/v1/keys", app.server_url))
        .header("Authorization", format!("Bearer {}", user.token))
        .json(&takeover_payload)
        .send()
        .await
        .unwrap();
    assert_eq!(takeover_resp.status(), 200);

    // 6. Verify Disconnect (Device A)
    // The Refactored WS client will stop receiving when closed.
    let env = ws.receive_envelope_timeout(std::time::Duration::from_millis(1000)).await;
    assert!(env.is_none(), "WebSocket should have been closed/stopped after takeover");

    // 7. Verify Cleanup
    app.assert_message_count(user.user_id, 0).await;

    let key_repo = KeyRepository::new();
    let mut conn = app.pool.acquire().await.unwrap();
    let bundle = key_repo.fetch_pre_key_bundle(&mut conn, user.user_id).await.unwrap().unwrap();

    use obscura_server::core::crypto_types::PublicKey;
    assert_eq!(bundle.identity_key, PublicKey::Edwards(new_ik_pub));
    assert_eq!(bundle.signed_pre_key.key_id, 2);
    assert_eq!(bundle.one_time_pre_key.unwrap().key_id, 10);
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

    let refill_payload = json!({
        "identityKey": base64::engine::general_purpose::STANDARD.encode(user.identity_key.verifying_key().to_bytes()),
        "registrationId": 111,
        "signedPreKey": {
            "keyId": 2,
            "publicKey": base64::engine::general_purpose::STANDARD.encode(&new_spk_pub),
            "signature": base64::engine::general_purpose::STANDARD.encode(&new_spk_sig)
        },
        "oneTimePreKeys": [
            { "keyId": 10, "publicKey": "QUFBQUFBQUFBQUFBQUFBQUFBQUFBQUFBQUFBQUFBQUE=" }
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
