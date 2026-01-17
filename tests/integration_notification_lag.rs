use base64::Engine;
use futures::StreamExt;
use obscura_server::proto::obscura::v1::EncryptedMessage;
use obscura_server::{api::app_router, core::notification::InMemoryNotifier};
use prost::Message as ProstMessage;
use serde_json::json;
use std::sync::Arc;
use std::time::Duration;
use tokio::net::TcpListener;
use tokio_tungstenite::connect_async;
use uuid::Uuid;

mod common;

#[tokio::test]
async fn test_slow_consumer_notification_drop() {
    // 1. Setup Server
    let pool = common::get_test_pool().await;
    let config = common::get_test_config();
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

    // 1. Register User A (Sender)
    let user_a_name = format!("alice_{}", run_id);
    let reg_a = json!({
        "username": user_a_name,
        "password": "password",
        "registrationId": 1,
        "identityKey": "dGVzdF9pZGVudGl0eV9rZXk=",
        "signedPreKey": {
            "keyId": 1,
            "publicKey": "dGVzdF9zaWduZWRfcHViX2tleQ==",
            "signature": "dGVzdF9zaWduZWRfc2ln"
        },
        "oneTimePreKeys": []
    });

    let resp_a = client.post(format!("{}/v1/accounts", server_url)).json(&reg_a).send().await.unwrap();
    let token_a = resp_a.json::<serde_json::Value>().await.unwrap()["token"].as_str().unwrap().to_string();

    // 2. Register User B (Receiver)
    let user_b_name = format!("bob_{}", run_id);
    let reg_b = json!({
        "username": user_b_name,
        "password": "password",
        "registrationId": 2,
        "identityKey": "dGVzdF9pZGVudGl0eV9rZXk=",
        "signedPreKey": {
            "keyId": 1,
            "publicKey": "dGVzdF9zaWduZWRfcHViX2tleQ==",
            "signature": "dGVzdF9zaWduZWRfc2ln"
        },
        "oneTimePreKeys": []
    });

    let resp_b = client.post(format!("{}/v1/accounts", server_url)).json(&reg_b).send().await.unwrap();
    let token_b = resp_b.json::<serde_json::Value>().await.unwrap()["token"].as_str().unwrap().to_string();
    let claims_b = decode_jwt_claims(&token_b);
    let user_b_id = claims_b["sub"].as_str().unwrap();

    // 3. Connect User B via WebSocket
    let (mut ws_stream, _) =
        connect_async(format!("{}?token={}", ws_url, token_b)).await.expect("Failed to connect WS");

    // IMPORTANT: We do NOT read from ws_stream yet.
    // This fills the TCP buffer, causing the server's `socket.send().await` to block.
    // While blocked, the notification channel (capacity 16) will fill up and overflow.

    // 4. Flood User B with 30 messages (exceeding channel capacity of 16)
    let message_count = 30;
    for i in 0..message_count {
        let outgoing =
            EncryptedMessage { r#type: 1, content: format!("Message {}", i).into_bytes() };
        let mut buf = Vec::new();
        outgoing.encode(&mut buf).unwrap();

        let resp = client
            .post(format!("{}/v1/messages/{}", server_url, user_b_id))
            .header("Authorization", format!("Bearer {}", token_a))
            .header("Content-Type", "application/octet-stream")
            .body(buf)
            .send()
            .await
            .unwrap();

        assert_eq!(resp.status(), 201, "Failed to send message {}", i);

        // Small yield to ensure server processes the request and tries to notify
        tokio::time::sleep(Duration::from_millis(5)).await;
    }

    // 5. Now start reading from WebSocket
    // We expect to receive ALL 30 messages.
    // If Lagged error was ignored, we would miss messages.
    // With Lagged handling, the server should fetch from DB and recover.

    let mut received_count = 0;
    let timeout = Duration::from_secs(5);

    let start = std::time::Instant::now();

    loop {
        if std::time::Instant::now().duration_since(start) > timeout {
            break;
        }

        if let Ok(Some(msg)) = tokio::time::timeout(Duration::from_millis(500), ws_stream.next()).await {
            match msg {
                Ok(tokio_tungstenite::tungstenite::protocol::Message::Binary(bin)) => {
                    use obscura_server::proto::obscura::v1::{WebSocketFrame, web_socket_frame::Payload};
                    if let Ok(frame) = WebSocketFrame::decode(bin.as_ref())
                        && let Some(Payload::Envelope(_)) = frame.payload {
                            received_count += 1;
                            if received_count == message_count {
                                break;
                            }
                    }
                }
                Ok(tokio_tungstenite::tungstenite::protocol::Message::Close(_)) | Ok(tokio_tungstenite::tungstenite::protocol::Message::Frame(_)) => break,
                Err(_) | Ok(tokio_tungstenite::tungstenite::protocol::Message::Ping(_)) | Ok(tokio_tungstenite::tungstenite::protocol::Message::Pong(_)) | Ok(tokio_tungstenite::tungstenite::protocol::Message::Text(_)) => {}
            }
        } else if std::time::Instant::now().duration_since(start) > timeout {
             break;
        }
    }

    assert_eq!(
        received_count, message_count,
        "Should receive all {} messages, but got {}",
        message_count, received_count
    );
}

fn decode_jwt_claims(token: &str) -> serde_json::Value {
    let parts: Vec<&str> = token.split('.').collect();
    let payload = parts[1];
    let decoded = base64::engine::general_purpose::URL_SAFE_NO_PAD.decode(payload).unwrap();
    serde_json::from_slice(&decoded).unwrap()
}
