use base64::Engine;
use futures::{SinkExt, StreamExt};
use obscura_server::{
    api::app_router, core::notification::InMemoryNotifier,
    proto::obscura::v1::EncryptedMessage,
};
use prost::Message as ProstMessage;
use std::sync::Arc;
use tokio::net::TcpListener;
use tokio_tungstenite::{connect_async, tungstenite::protocol::Message};
use uuid::Uuid;

mod common;

#[tokio::test]
async fn test_ping_pong_under_load() {
    let pool = common::get_test_pool().await;
    let mut config = common::get_test_config();

    // Use a large batch limit to ensure we hit backpressure if the loop is blocking
    config.message_batch_limit = 100;

    let notifier = Arc::new(InMemoryNotifier::new(config.clone()));
    let app = app_router(pool, config.clone(), notifier);

    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();
    let server_url = format!("http://{}", addr);
    let ws_url = format!("ws://{}/v1/gateway", addr);

    tokio::spawn(async move {
        axum::serve(listener, app.into_make_service_with_connect_info::<std::net::SocketAddr>()).await.unwrap();
    });

    let client = reqwest::Client::new();
    let run_id = Uuid::new_v4().to_string()[..8].to_string();

    let user_a_name = format!("alice_{}", run_id);
    let token_a = register_user(&client, &server_url, &user_a_name, 1).await;
    let user_b_name = format!("bob_{}", run_id);
    let token_b = register_user(&client, &server_url, &user_b_name, 2).await;
    let claims_b = decode_jwt_claims(&token_b);
    let user_b_id = claims_b["sub"].as_str().unwrap();

    // 1. Fill Inbox with LARGE messages to fill TCP buffer
    // 100 messages * 500KB = 50MB.
    let large_payload = vec![0u8; 1024 * 500];
    for _ in 0..100 {
        send_message(&client, &server_url, &token_a, user_b_id, &large_payload).await;
    }

    // 2. Connect via WebSocket
    let (mut ws_stream, _) =
        connect_async(format!("{}?token={}", ws_url, token_b)).await.expect("Failed to connect WS");

    // 3. Confirm server has started flushing
    match ws_stream.next().await {
        Some(Ok(Message::Binary(_))) => {}
        _ => panic!("Expected initial binary message"),
    }

    // 4. Send a PING.
    ws_stream.send(Message::Ping(vec![1, 2, 3].into())).await.unwrap();

    // 5. Verify the Pong arrives QUICKLY (before the whole batch finishes)
    let mut binary_count = 0;
    let mut pong_received = false;
    let timeout = tokio::time::Duration::from_secs(5);
    let start = std::time::Instant::now();

    while start.elapsed() < timeout {
        match tokio::time::timeout(tokio::time::Duration::from_millis(500), ws_stream.next()).await {
            Ok(Some(Ok(msg))) => {
                match msg {
                    Message::Pong(payload) => {
                        assert_eq!(payload, vec![1, 2, 3]);
                        pong_received = true;
                        break;
                    }
                    Message::Binary(_) => {
                        binary_count += 1;
                    }
                    _ => {}
                }
            }
            _ => break,
        }
    }

    assert!(pong_received, "Did not receive Pong under load");
    // If truly concurrent, the Ping should be processed almost immediately at the application layer.
    // However, we allow up to 40 binary messages to account for:
    // 1. OS-level TCP send buffers (which can hold several MBs of our 500KB frames).
    // 2. CI environment scheduling latency.
    //
    // The key is that we receive the Pong WELL before the 100-message backlog is finished,
    // proving the server is not blocked on a single sequential loop.
    assert!(binary_count < 40, "Server blocked! Received {} binary messages before Pong", binary_count);
}

async fn register_user(client: &reqwest::Client, server_url: &str, username: &str, reg_id: u32) -> String {
    let reg = serde_json::json!({
        "username": username,
        "password": "password",
        "registrationId": reg_id,
        "identityKey": "dGVzdF9pZGVudGl0eV9rZXk=",
        "signedPreKey": {
            "keyId": 1, "publicKey": "dGVzdF9zaWduZWRfcHViX2tleQ==", "signature": "dGVzdF9zaWduZWRfc2ln"
        },
        "oneTimePreKeys": []
    });
    let resp = client.post(format!("{}/v1/accounts", server_url)).json(&reg).send().await.unwrap();
    assert_eq!(resp.status(), 201, "User registration failed");
    resp.json::<serde_json::Value>().await.unwrap()["token"].as_str().unwrap().to_string()
}

async fn send_message(client: &reqwest::Client, server_url: &str, token: &str, recipient_id: &str, content: &[u8]) {
    let enc_msg = EncryptedMessage {
        r#type: 2,
        content: content.to_vec(),
    };
    let mut buf = Vec::new();
    enc_msg.encode(&mut buf).unwrap();

    let resp = client
        .post(format!("{}/v1/messages/{}", server_url, recipient_id))
        .header("Authorization", format!("Bearer {}", token))
        .header("Content-Type", "application/octet-stream")
        .body(buf)
        .send()
        .await
        .unwrap();
    assert_eq!(resp.status(), 201, "Message sending failed in load test");
}

fn decode_jwt_claims(token: &str) -> serde_json::Value {
    let parts: Vec<&str> = token.split('.').collect();
    let payload = parts[1];
    let decoded = base64::engine::general_purpose::URL_SAFE_NO_PAD.decode(payload).unwrap();
    serde_json::from_slice(&decoded).unwrap()
}