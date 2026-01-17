use base64::Engine;
use futures::StreamExt;
use obscura_server::{api::app_router, core::notification::InMemoryNotifier};
use prost::Message as ProstMessage;
use serde_json::json;
use std::sync::Arc;
use tokio::net::TcpListener;
use tokio_tungstenite::{connect_async, tungstenite::protocol::Message};
use uuid::Uuid;
use obscura_server::proto::obscura::v1::{WebSocketFrame, web_socket_frame::Payload};

mod common;

/// Verifies that the server correctly recovers from a "Lagged" notification state.
///
/// In this scenario:
/// 1. A client is connected but not reading from the WebSocket (simulating extreme backpressure).
/// 2. The server receives more messages than the notification channel's capacity.
/// 3. The notification channel overflows (drops events).
/// 4. The server's delivery task detects the lag and performs a full DB poll to recover.
/// 5. The client eventually reads all messages, ensuring zero message loss despite the lag.
#[tokio::test]
async fn test_notification_lag_recovery() {
    let pool = common::get_test_pool().await;
    let mut config = common::get_test_config();
    
    // Set a very small buffer to force a lag even with a small number of messages
    config.ws_outbound_buffer_size = 10;

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

    let user_a_name = format!("alice_{}", run_id);
    let token_a = register_user(&client, &server_url, &user_a_name, 1).await;
    let user_b_name = format!("bob_{}", run_id);
    let token_b = register_user(&client, &server_url, &user_b_name, 2).await;
    let claims_b = decode_jwt_claims(&token_b);
    let user_b_id = claims_b["sub"].as_str().unwrap();

    // Connect User B but DO NOT read from the stream initially
    let (mut ws_stream, _) =
        connect_async(format!("{}?token={}", ws_url, token_b)).await.expect("Failed to connect WS");

    // Flood the system with messages to fill the notification channel
    let message_count = 100;
    for i in 0..message_count {
        let content = format!("Message {}", i).into_bytes();
        client
            .post(format!("{}/v1/messages/{}", server_url, user_b_id))
            .header("Authorization", format!("Bearer {}", token_a))
            .header("Content-Type", "application/octet-stream")
            .body(content)
            .send()
            .await
            .unwrap();
    }

    // Allow time for notifications to propagate and overflow
    tokio::time::sleep(tokio::time::Duration::from_millis(500)).await;

    // Start reading everything from the WebSocket
    let mut received = 0;
    while let Ok(Some(Ok(Message::Binary(bin)))) = tokio::time::timeout(std::time::Duration::from_millis(100), ws_stream.next()).await {
        let frame = WebSocketFrame::decode(bin.as_ref()).unwrap();
        if let Some(Payload::Envelope(_)) = frame.payload {
            received += 1;
        }
    }

    // Verify that the server's lag-recovery logic successfully delivered ALL messages
    assert_eq!(received, message_count, "Should receive all {} messages despite notification lag", message_count);
}

async fn register_user(client: &reqwest::Client, server_url: &str, username: &str, reg_id: u32) -> String {
    let reg = json!({
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

fn decode_jwt_claims(token: &str) -> serde_json::Value {
    let parts: Vec<&str> = token.split('.').collect();
    let payload = parts[1];
    let decoded = base64::engine::general_purpose::URL_SAFE_NO_PAD.decode(payload).unwrap();
    serde_json::from_slice(&decoded).unwrap()
}
