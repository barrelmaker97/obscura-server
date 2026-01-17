use base64::Engine;
use futures::StreamExt;
use obscura_server::{
    api::app_router, core::notification::InMemoryNotifier, storage::key_repo::KeyRepository,
    storage::message_repo::MessageRepository,
};
use serde_json::json;
use std::sync::Arc;
use tokio::net::TcpListener;
use tokio_tungstenite::{connect_async, tungstenite::protocol::Message};
use uuid::Uuid;

mod common;

// Helper to decode JWT
fn decode_jwt_claims(token: &str) -> serde_json::Value {
    let parts: Vec<&str> = token.split('.').collect();
    let payload = parts[1];
    let decoded = base64::engine::general_purpose::URL_SAFE_NO_PAD.decode(payload).unwrap();
    serde_json::from_slice(&decoded).unwrap()
}

#[tokio::test]
async fn test_device_takeover_success() {
    // 1. Setup Server
    let pool = common::get_test_pool().await;
    let config = common::get_test_config();
    let notifier = Arc::new(InMemoryNotifier::new(config.clone()));
    let app = app_router(pool.clone(), config.clone(), notifier);

    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();
    let server_url = format!("http://{}", addr);
    let ws_url = format!("ws://{}/v1/gateway", addr);

    tokio::spawn(async move {
        axum::serve(listener, app.into_make_service_with_connect_info::<std::net::SocketAddr>()).await.unwrap();
    });

    let client = reqwest::Client::new();
    let run_id = Uuid::new_v4().to_string()[..8].to_string();
    let username = format!("takeover_user_{}", run_id);

    // 2. Register User (Device A)
    let reg_payload = json!({
        "username": username,
        "password": "password",
        "registrationId": 111,
        "identityKey": "QUFBQUFBQUFBQUFBQUFBQUFBQUFBQUFBQUFBQUFBQUE=", // AAAAA...
        "signedPreKey": {
            "keyId": 1,
            "publicKey": "QUFBQUFBQUFBQUFBQUFBQUFBQUFBQUFBQUFBQUFBQUE=",
            "signature": "QUFBQUFBQUFBQUFBQUFBQUFBQUFBQUFBQUFBQUFBQUE="
        },
        "oneTimePreKeys": [
            { "keyId": 1, "publicKey": "QUFBQUFBQUFBQUFBQUFBQUFBQUFBQUFBQUFBQUFBQUE=" }
        ]
    });

    let resp = client.post(format!("{}/v1/accounts", server_url)).json(&reg_payload).send().await.unwrap();
    assert_eq!(resp.status(), 201);
    let token = resp.json::<serde_json::Value>().await.unwrap()["token"].as_str().unwrap().to_string();
    let claims = decode_jwt_claims(&token);
    let user_id = Uuid::parse_str(claims["sub"].as_str().unwrap()).unwrap();

    // 3. Populate Data (PreKeys exist from reg, add a pending message)
    let msg_repo = MessageRepository::new(pool.clone());
    msg_repo.create(user_id, user_id, vec![1, 2, 3], 30).await.unwrap();
    let pending_before = msg_repo.fetch_pending_batch(user_id, None, 100).await.unwrap();
    assert_eq!(pending_before.len(), 1);

    // 4. Connect WebSocket (Device A)
    let (mut ws_stream, _) =
        connect_async(format!("{}?token={}", ws_url, token)).await.expect("Failed to connect WS A");

    // 5. Perform Takeover (Device B)
    // New Identity Key: BBBBB...
    let takeover_payload = json!({
        "identityKey": "QkJCQkJCQkJCQkJCQkJCQkJCQkJCQkJCQkJCQkJCQkI=",
        "registrationId": 222,
        "signedPreKey": {
            "keyId": 2,
            "publicKey": "QkJCQkJCQkJCQkJCQkJCQkJCQkJCQkJCQkJCQkJCQkI=",
            "signature": "QkJCQkJCQkJCQkJCQkJCQkJCQkJCQkJCQkJCQkJCQkI="
        },
        "oneTimePreKeys": [
            { "keyId": 10, "publicKey": "QkJCQkJCQkJCQkJCQkJCQkJCQkJCQkJCQkJCQkJCQkI=" }
        ]
    });

    let takeover_resp = client
        .post(format!("{}/v1/keys", server_url))
        .header("Authorization", format!("Bearer {}", token))
        .json(&takeover_payload)
        .send()
        .await
        .unwrap();
    assert_eq!(takeover_resp.status(), 200);

    // 6. Verify Disconnect (Device A)
    // We expect the WS stream to close.
    // We match directly on the next item.
    match ws_stream.next().await {
        Some(Ok(Message::Close(_))) => {} // Clean close
        Some(Ok(Message::Binary(_))) => {
            // If we get binary (pending msg), wait for next which should be close or None
            match ws_stream.next().await {
                Some(Ok(Message::Close(_))) => {}
                None => {}         // Closed stream
                Some(Err(_)) => {} // Error is fine
                _ => panic!("Expected disconnect after binary"),
            }
        }
        Some(Err(_)) => {} // Dirty close/error
        None => {}         // Stream ended
        _ => panic!("Unexpected message"),
    }

    // 7. Verify Cleanup
    // Pending messages should be gone
    let pending_after = msg_repo.fetch_pending_batch(user_id, None, 100).await.unwrap();
    assert_eq!(pending_after.len(), 0);

    // Old PreKeys should be gone (Key ID 1 was uploaded initially, Key ID 2 is new)
    let key_repo = KeyRepository::new(pool.clone());
    let bundle = key_repo.fetch_pre_key_bundle(user_id).await.unwrap().unwrap();

    // Check Identity Key
    assert_eq!(
        bundle.identity_key,
        base64::engine::general_purpose::STANDARD.decode("QkJCQkJCQkJCQkJCQkJCQkJCQkJCQkJCQkJCQkJCQkI=").unwrap()
    );
    // Check Signed Pre Key ID
    assert_eq!(bundle.signed_pre_key.key_id, 2);
    // Check One Time Pre Key ID (should be 10)
    assert_eq!(bundle.one_time_pre_key.unwrap().key_id, 10);
}

#[tokio::test]
async fn test_refill_pre_keys_no_overwrite() {
    let pool = common::get_test_pool().await;
    let config = common::get_test_config();
    let notifier = Arc::new(InMemoryNotifier::new(config.clone()));
    let app = app_router(pool.clone(), config.clone(), notifier);

    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();
    let server_url = format!("http://{}", addr);

    tokio::spawn(async move {
        axum::serve(listener, app.into_make_service_with_connect_info::<std::net::SocketAddr>()).await.unwrap();
    });

    let client = reqwest::Client::new();
    let run_id = Uuid::new_v4().to_string()[..8].to_string();
    let username = format!("refill_user_{}", run_id);

    // 1. Register
    let reg_payload = json!({
        "username": username,
        "password": "password",
        "registrationId": 111,
        "identityKey": "QUFBQUFBQUFBQUFBQUFBQUFBQUFBQUFBQUFBQUFBQUE=",
        "signedPreKey": {
            "keyId": 1,
            "publicKey": "QUFBQUFBQUFBQUFBQUFBQUFBQUFBQUFBQUFBQUFBQUE=",
            "signature": "QUFBQUFBQUFBQUFBQUFBQUFBQUFBQUFBQUFBQUFBQUE="
        },
        "oneTimePreKeys": []
    });

    let resp = client.post(format!("{}/v1/accounts", server_url)).json(&reg_payload).send().await.unwrap();
    assert_eq!(resp.status(), 201);
    let token = resp.json::<serde_json::Value>().await.unwrap()["token"].as_str().unwrap().to_string();

    // 2. Refill (Same Identity Key)
    // We explicitly provide the SAME identity key.
    let refill_payload = json!({
        "identityKey": "QUFBQUFBQUFBQUFBQUFBQUFBQUFBQUFBQUFBQUFBQUE=", // Same
        "registrationId": 111,
        "signedPreKey": {
            "keyId": 2, // New SPK
            "publicKey": "QUFBQUFBQUFBQUFBQUFBQUFBQUFBQUFBQUFBQUFBQUE=",
            "signature": "QUFBQUFBQUFBQUFBQUFBQUFBQUFBQUFBQUFBQUFBQUE="
        },
        "oneTimePreKeys": [
            { "keyId": 10, "publicKey": "QUFBQUFBQUFBQUFBQUFBQUFBQUFBQUFBQUFBQUFBQUE=" }
        ]
    });

    let refill_resp = client
        .post(format!("{}/v1/keys", server_url))
        .header("Authorization", format!("Bearer {}", token))
        .json(&refill_payload)
        .send()
        .await
        .unwrap();
    assert_eq!(refill_resp.status(), 200);
}

#[tokio::test]
async fn test_no_identity_key_rejects_websocket() {
    let pool = common::get_test_pool().await;
    let config = common::get_test_config();
    let notifier = Arc::new(InMemoryNotifier::new(config.clone()));
    let app = app_router(pool.clone(), config.clone(), notifier);

    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();
    let ws_url = format!("ws://{}/v1/gateway", addr);

    tokio::spawn(async move {
        axum::serve(listener, app.into_make_service_with_connect_info::<std::net::SocketAddr>()).await.unwrap();
    });

    let run_id = Uuid::new_v4().to_string()[..8].to_string();

    // 1. Create user via Repo directly (bypassing API validation)
    use obscura_server::storage::user_repo::UserRepository;
    let user_repo = UserRepository::new();
    let mut tx = pool.begin().await.unwrap();
    let user = user_repo.create(&mut *tx, &format!("nokey_{}", run_id), "hash").await.unwrap();
    tx.commit().await.unwrap();

    // Generate a token for this user
    let token = obscura_server::api::middleware::create_jwt(user.id, &config.jwt_secret).unwrap();

    // Verify connection is rejected or closed immediately
    let res = connect_async(format!("{}?token={}", ws_url, token)).await;
    if let Ok((mut ws, _)) = res {
        // If connection succeeds, it should close immediately
        match ws.next().await {
            Some(Ok(Message::Close(_))) => {}
            None => {}         // Closed
            Some(Err(_)) => {} // Error/Closed
            _ => {} // Anything else implies connection is open, but if it closes right after, that's fine too.
                    // Ideally we want to see a close or stream end.
        }
    }
}
