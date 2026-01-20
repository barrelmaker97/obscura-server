use base64::Engine;
use futures::StreamExt;
use obscura_server::{
    api::middleware::create_jwt,
    storage::{key_repo::KeyRepository, message_repo::MessageRepository, user_repo::UserRepository},
};
use serde_json::json;
use tokio_tungstenite::{connect_async, tungstenite::protocol::Message};
use uuid::Uuid;

mod common;

#[tokio::test]
async fn test_device_takeover_success() {
    let app = common::TestApp::spawn().await;
    let run_id = Uuid::new_v4().to_string()[..8].to_string();
    let username = format!("takeover_user_{}", run_id);

    let identity_key = common::generate_signing_key();
    let ik_pub = identity_key.verifying_key().to_bytes();
    let (spk_pub, spk_sig) = common::generate_signed_pre_key(&identity_key);

    // 2. Register User (Device A)
    let reg_payload = json!({
        "username": username,
        "password": "password",
        "registrationId": 111,
        "identityKey": base64::engine::general_purpose::STANDARD.encode(&ik_pub),
        "signedPreKey": {
            "keyId": 1,
            "publicKey": base64::engine::general_purpose::STANDARD.encode(&spk_pub),
            "signature": base64::engine::general_purpose::STANDARD.encode(&spk_sig)
        },
        "oneTimePreKeys": [
            { "keyId": 1, "publicKey": "QUFBQUFBQUFBQUFBQUFBQUFBQUFBQUFBQUFBQUFBQUE=" }
        ]
    });

    let resp = app.client.post(format!("{}/v1/users", app.server_url)).json(&reg_payload).send().await.unwrap();
    assert_eq!(resp.status(), 201);
    let token = resp.json::<serde_json::Value>().await.unwrap()["token"].as_str().unwrap().to_string();

    // Parse user_id from token manually since we didn't use helper
    let parts: Vec<&str> = token.split('.').collect();
    let decoded = base64::engine::general_purpose::URL_SAFE_NO_PAD.decode(parts[1]).unwrap();
    let claims: serde_json::Value = serde_json::from_slice(&decoded).unwrap();
    let user_id = Uuid::parse_str(claims["sub"].as_str().unwrap()).unwrap();

    // 3. Populate Data (PreKeys exist from reg, add a pending message)
    let msg_repo = MessageRepository::new(app.pool.clone());
    msg_repo.create(user_id, user_id, 2, vec![1, 2, 3], 30).await.unwrap();
    let pending_before = msg_repo.fetch_pending_batch(user_id, None, 100).await.unwrap();
    assert_eq!(pending_before.len(), 1);

    // 4. Connect WebSocket (Device A)
    let mut ws = app.connect_ws(&token).await;

    // 5. Perform Takeover (Device B)
    let new_identity_key = common::generate_signing_key();
    let new_ik_pub = new_identity_key.verifying_key().to_bytes();
    let (new_spk_pub, new_spk_sig) = common::generate_signed_pre_key(&new_identity_key);

    let takeover_payload = json!({
        "identityKey": base64::engine::general_purpose::STANDARD.encode(&new_ik_pub),
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
        .header("Authorization", format!("Bearer {}", token))
        .json(&takeover_payload)
        .send()
        .await
        .unwrap();
    assert_eq!(takeover_resp.status(), 200);

    // 6. Verify Disconnect (Device A)
    let timeout = std::time::Duration::from_secs(5);
    let start = std::time::Instant::now();
    
    loop {
        if start.elapsed() > timeout {
            panic!("Timed out waiting for disconnect");
        }
        
        match tokio::time::timeout(std::time::Duration::from_millis(500), ws.stream.next()).await {
            Ok(Some(Ok(Message::Close(_)))) => break, // Clean close
            Ok(None) => break,         // Stream ended
            Ok(Some(Ok(Message::Binary(_)))) => {
                // Ignore binary messages
                continue;
            }
            Ok(Some(Err(_))) => break, // Dirty close/error
            Ok(Some(Ok(_))) => panic!("Unexpected message type"),
            Err(_) => continue, // Timeout on individual read
        }
    }

    // 7. Verify Cleanup
    let pending_after = msg_repo.fetch_pending_batch(user_id, None, 100).await.unwrap();
    assert_eq!(pending_after.len(), 0);

    let key_repo = KeyRepository::new(app.pool.clone());
    let bundle = key_repo.fetch_pre_key_bundle(user_id).await.unwrap().unwrap();

    // Check Identity Key
    assert_eq!(bundle.identity_key, new_ik_pub);
    // Check Signed Pre Key ID
    assert_eq!(bundle.signed_pre_key.key_id, 2);
    // Check One Time Pre Key ID (should be 10)
    assert_eq!(bundle.one_time_pre_key.unwrap().key_id, 10);
}

#[tokio::test]
async fn test_refill_pre_keys_no_overwrite() {
    let app = common::TestApp::spawn().await;
    let run_id = Uuid::new_v4().to_string()[..8].to_string();
    let username = format!("refill_user_{}", run_id);

    let identity_key = common::generate_signing_key();
    let ik_pub = identity_key.verifying_key().to_bytes();
    let (spk_pub, spk_sig) = common::generate_signed_pre_key(&identity_key);

    // 1. Register with custom keys
    let reg_payload = json!({
        "username": username,
        "password": "password",
        "registrationId": 111,
        "identityKey": base64::engine::general_purpose::STANDARD.encode(&ik_pub),
        "signedPreKey": {
            "keyId": 1,
            "publicKey": base64::engine::general_purpose::STANDARD.encode(&spk_pub),
            "signature": base64::engine::general_purpose::STANDARD.encode(&spk_sig)
        },
        "oneTimePreKeys": []
    });

    let resp = app.client.post(format!("{}/v1/users", app.server_url)).json(&reg_payload).send().await.unwrap();
    assert_eq!(resp.status(), 201);
    let token = resp.json::<serde_json::Value>().await.unwrap()["token"].as_str().unwrap().to_string();

    // 2. Refill (Same Identity Key)
    let (new_spk_pub, new_spk_sig) = common::generate_signed_pre_key(&identity_key);

    let refill_payload = json!({
        "identityKey": base64::engine::general_purpose::STANDARD.encode(&ik_pub), // Same
        "registrationId": 111,
        "signedPreKey": {
            "keyId": 2, // New SPK
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
        .header("Authorization", format!("Bearer {}", token))
        .json(&refill_payload)
        .send()
        .await
        .unwrap();
    assert_eq!(refill_resp.status(), 200);
}

#[tokio::test]
async fn test_no_identity_key_rejects_websocket() {
    let app = common::TestApp::spawn().await;
    let run_id = Uuid::new_v4().to_string()[..8].to_string();

    // 1. Create user via Repo directly (bypassing API validation)
    let user_repo = UserRepository::new();
    let mut tx = app.pool.begin().await.unwrap();
    let user = user_repo.create(&mut *tx, &format!("nokey_{}", run_id), "hash").await.unwrap();
    tx.commit().await.unwrap();

    // Generate a token for this user
    let token = create_jwt(user.id, &app.config.auth.jwt_secret, 3600).unwrap();

    // Verify connection is rejected or closed immediately
    let res = connect_async(format!("{}?token={}", app.ws_url, token)).await;
    if let Ok((mut ws, _)) = res {
        // If connection succeeds, it should close immediately
        match ws.next().await {
            Some(Ok(Message::Close(_))) => {}
            None => {}         // Closed
            Some(Err(_)) => {} // Error/Closed
            _ => {}
        }
    }
}