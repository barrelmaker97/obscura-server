use base64::Engine;
use futures::{SinkExt, StreamExt};
use obscura_server::{
    api::app_router, core::notification::InMemoryNotifier, proto::obscura::v1::OutgoingMessage,
};
use prost::Message as ProstMessage;
use std::sync::Arc;
use tokio::net::TcpListener;
use tokio_tungstenite::{connect_async, tungstenite::protocol::Message};
use uuid::Uuid;

mod common;

#[tokio::test]
async fn test_ping_pong_under_load() {
    // 1. Setup Server
    let pool = common::get_test_pool().await;
    let mut config = common::get_test_config();
    // Batch limit larger than internal channel (32) to ensure we hit backpressure
    // if the TCP buffer fills.
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

    // 2. Register Users
    let user_a_name = format!("alice_{}", run_id);
    let token_a = register_user(&client, &server_url, &user_a_name, 1).await;
    let user_b_name = format!("bob_{}", run_id);
    let token_b = register_user(&client, &server_url, &user_b_name, 2).await;
    let claims_b = decode_jwt_claims(&token_b);
    let user_b_id = claims_b["sub"].as_str().unwrap();

    // 3. Fill Inbox with LARGE messages to fill TCP buffer quickly
    // 100 messages * 500KB = 50MB.
    let large_payload = vec![0u8; 1024 * 500]; // 500KB payload
    for _ in 0..100 {
        send_message(&client, &server_url, &token_a, user_b_id, &large_payload).await;
    }

    // 4. Connect with WebSocket
    let (mut ws_stream, _) =
        connect_async(format!("{}?token={}", ws_url, token_b)).await.expect("Failed to connect WS");

    // 5. Read ONE message to confirm the server has started flushing
    match ws_stream.next().await {
        Some(Ok(Message::Binary(_))) => {}
        _ => panic!("Expected initial binary message"),
    }

    // 6. Send a PING.
    ws_stream.send(Message::Ping(vec![1, 2, 3].into())).await.unwrap();

    // 7. Measure how many binary messages arrive before the PONG.
    // On a blocking server, it must finish the batch (or at least the current loop)
    // before it even checks for the Ping.
    let mut binary_count = 0;
    let mut pong_received = false;
    let timeout = tokio::time::Duration::from_secs(5);
    let start = tokio::time::Instant::now();

    while start.elapsed() < timeout {
        match tokio::time::timeout(tokio::time::Duration::from_millis(500), ws_stream.next()).await {
            Ok(Some(Ok(msg))) => {
                match msg {
                    Message::Pong(payload) => {
                        if payload == vec![1, 2, 3] {
                            pong_received = true;
                            break;
                        }
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

    println!("Binary messages received before Pong: {}", binary_count);
    assert!(pong_received, "Did not receive Pong");

    // On a blocking implementation with 100 messages, the server would have sent
    // most of them before seeing the Ping.
    // If it's truly concurrent, the Ping should be processed almost immediately.
    assert!(binary_count < 10, "Server blocked! Received {} binary messages before Pong", binary_count);
}

async fn register_user(client: &reqwest::Client, server_url: &str, username: &str, reg_id: u32) -> String {
    let reg = serde_json::json!({
        "username": username,
        "password": "password",
        "registrationId": reg_id,
        "identityKey": "dGVzdF9pZGVudGl0eV9rZXk=",
        "signedPreKey": {
            "keyId": 1,
            "publicKey": "dGVzdF9zaWduZWRfcHViX2tleQ==",
            "signature": "dGVzdF9zaWduZWRfc2ln"
        },
        "oneTimePreKeys": []
    });
    let resp = client.post(format!("{}/v1/accounts", server_url)).json(&reg).send().await.unwrap();
    resp.json::<serde_json::Value>().await.unwrap()["token"].as_str().unwrap().to_string()
}

async fn send_message(client: &reqwest::Client, server_url: &str, token: &str, recipient_id: &str, content: &[u8]) {
    let outgoing = OutgoingMessage { r#type: 1, content: content.to_vec() };
    let mut buf = Vec::new();
    outgoing.encode(&mut buf).unwrap();

    client
        .post(format!("{}/v1/messages/{}", server_url, recipient_id))
        .header("Authorization", format!("Bearer {}", token))
        .header("Content-Type", "application/octet-stream")
        .body(buf)
        .send()
        .await
        .unwrap();
}

fn decode_jwt_claims(token: &str) -> serde_json::Value {
    let parts: Vec<&str> = token.split('.').collect();
    let payload = parts[1];
    let decoded = base64::engine::general_purpose::URL_SAFE_NO_PAD.decode(payload).unwrap();
    serde_json::from_slice(&decoded).unwrap()
}
