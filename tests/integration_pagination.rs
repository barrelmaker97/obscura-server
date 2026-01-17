use base64::Engine;
use futures::StreamExt;
use obscura_server::proto::obscura::v1::{WebSocketFrame, web_socket_frame::Payload};
use obscura_server::{api::app_router, core::notification::InMemoryNotifier};
use prost::Message as ProstMessage;
use serde_json::json;
use std::sync::Arc;
use tokio::net::TcpListener;
use tokio_tungstenite::{connect_async, tungstenite::protocol::Message};
use uuid::Uuid;

mod common;

#[tokio::test]
async fn test_message_pagination_large_backlog() {
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

    let user_a_name = format!("alice_{}", run_id);
    let token_a = register_user(&client, &server_url, &user_a_name, 1).await;
    let user_b_name = format!("bob_{}", run_id);
    let token_b = register_user(&client, &server_url, &user_b_name, 2).await;
    let claims_b = decode_jwt_claims(&token_b);
    let user_b_id = claims_b["sub"].as_str().unwrap();

    let message_count = 125;
    for i in 0..message_count {
        let content = format!("Message {}", i);
        send_message(&client, &server_url, &token_a, user_b_id, content.as_bytes()).await;
    }

    let (mut ws_stream, _) =
        connect_async(format!("{}?token={}", ws_url, token_b)).await.expect("Failed to connect WS");

    let mut received_messages = Vec::new();
    let mut received_ids = Vec::new();
    let timeout = std::time::Duration::from_secs(10);
    let start = std::time::Instant::now();

    while received_messages.len() < message_count && start.elapsed() < timeout {
        match tokio::time::timeout(std::time::Duration::from_millis(1000), ws_stream.next()).await {
            Ok(Some(Ok(Message::Binary(bin)))) => {
                let frame = WebSocketFrame::decode(bin.as_ref()).unwrap();
                if let Some(Payload::Envelope(env)) = frame.payload {
                    if !received_ids.contains(&env.id) {
                        received_ids.push(env.id);
                        received_messages.push(String::from_utf8(env.content).unwrap());
                    }
                }
            }
            _ => break,
        }
    }

    assert_eq!(received_messages.len(), message_count, "Did not receive all messages in pagination test");

    for (i, msg) in received_messages.iter().enumerate().take(message_count) {
        assert_eq!(msg.as_bytes(), format!("Message {}", i).as_bytes(), "Message content mismatch at index {}", i);
    }
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
    assert_eq!(resp.status(), 201, "User registration failed in pagination test");
    resp.json::<serde_json::Value>().await.unwrap()["token"].as_str().unwrap().to_string()
}

async fn send_message(client: &reqwest::Client, server_url: &str, token: &str, recipient_id: &str, content: &[u8]) {
    let resp = client
        .post(format!("{}/v1/messages/{}", server_url, recipient_id))
        .header("Authorization", format!("Bearer {}", token))
        .header("Content-Type", "application/octet-stream")
        .body(content.to_vec())
        .send()
        .await
        .unwrap();
    assert_eq!(resp.status(), 201, "Message sending failed in pagination test");
}

fn decode_jwt_claims(token: &str) -> serde_json::Value {
    let parts: Vec<&str> = token.split('.').collect();
    let payload = parts[1];
    let decoded = base64::engine::general_purpose::URL_SAFE_NO_PAD.decode(payload).unwrap();
    serde_json::from_slice(&decoded).unwrap()
}
