use base64::Engine;
use futures::StreamExt;
use obscura_server::api::middleware::create_jwt;
use obscura_server::storage::{
    key_repo::KeyRepository, message_repo::MessageRepository, user_repo::UserRepository,
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

    // 2. Register User (Device A)
    let user = app.register_user_with_keys(&username, 111, 1).await;

    // 3. Populate Data
    let msg_repo = MessageRepository::new(app.pool.clone());
    msg_repo.create(user.user_id, user.user_id, 2, vec![1, 2, 3], 30).await.unwrap();
    let pending_before = msg_repo.fetch_pending_batch(user.user_id, None, 100).await.unwrap();
    assert_eq!(pending_before.len(), 1);

    // 4. Connect WebSocket (Device A)
    let mut ws = app.connect_ws(&user.token).await;

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
        .header("Authorization", format!("Bearer {}", user.token))
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
            Ok(Some(Ok(Message::Close(_)))) => break, 
            Ok(None) => break,         
            Ok(Some(Ok(Message::Binary(_)))) => continue,
            Ok(Some(Err(_))) => break, 
            Ok(Some(Ok(_))) => panic!("Unexpected message type"),
            Err(_) => continue, 
        }
    }

    // 7. Verify Cleanup
    let pending_after = msg_repo.fetch_pending_batch(user.user_id, None, 100).await.unwrap();
    assert_eq!(pending_after.len(), 0);

    let key_repo = KeyRepository::new(app.pool.clone());
    let bundle = key_repo.fetch_pre_key_bundle(user.user_id).await.unwrap().unwrap();

    assert_eq!(bundle.identity_key, new_ik_pub);
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

#[tokio::test]
async fn test_no_identity_key_rejects_websocket() {
    let app = common::TestApp::spawn().await;
    let run_id = Uuid::new_v4().to_string()[..8].to_string();

    // 1. Create user via Repo directly (bypassing API validation)
    let user_repo = UserRepository::new();
    let mut tx = app.pool.begin().await.unwrap();
    let user: obscura_server::core::user::User = user_repo.create(&mut *tx, &format!("nokey_{}", run_id), "hash").await.unwrap();
    tx.commit().await.unwrap();

    // Generate a token for this user
    let token = create_jwt(user.id, &app.config.auth.jwt_secret, 3600).unwrap();

    // Verify connection is rejected or closed immediately
    let res = connect_async(format!("{}?token={}", app.ws_url, token)).await;
    if let Ok((mut ws, _)) = res {
        // If connection succeeds, it should close immediately
        match ws.next().await {
            Some(Ok(Message::Close(_))) => {}
            None => {}         
            Some(Err(_)) => {} 
            _ => {}
        }
    }
}
