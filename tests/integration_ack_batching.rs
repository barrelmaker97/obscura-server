use base64::Engine;
use futures::{SinkExt, StreamExt};
use obscura_server::{
    api::app_router, core::notification::InMemoryNotifier,
    proto::obscura::v1::{EncryptedMessage, WebSocketFrame, web_socket_frame::Payload, AckMessage},
};
use prost::Message as ProstMessage;
use std::sync::Arc;
use tokio::net::TcpListener;
use tokio_tungstenite::{connect_async, tungstenite::protocol::Message};
use uuid::Uuid;
use sqlx::Row;

mod common;

#[tokio::test]
async fn test_ack_batching_behavior() {
    let pool = common::get_test_pool().await;
    let mut config = common::get_test_config();
    config.ws_ack_batch_size = 5;
    config.ws_ack_flush_interval_ms = 1000;

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
    let user_b_id = Uuid::parse_str(claims_b["sub"].as_str().unwrap()).unwrap();

    for i in 0..3 {
        send_message(&client, &server_url, &token_a, &user_b_id.to_string(), format!("msg {}", i).as_bytes()).await;
    }

    let (mut ws_stream, _) =
        connect_async(format!("{}?token={}", ws_url, token_b)).await.expect("Failed to connect WS");

    let mut message_ids = Vec::new();
    for _ in 0..3 {
        if let Some(Ok(Message::Binary(bin))) = ws_stream.next().await {
            let frame = WebSocketFrame::decode(bin.as_ref()).unwrap();
            if let Some(Payload::Envelope(env)) = frame.payload {
                message_ids.push(env.id);
            }
        }
    }
    assert_eq!(message_ids.len(), 3);

    for id in &message_ids {
        let ack_frame = WebSocketFrame {
            payload: Some(Payload::Ack(AckMessage { message_id: id.clone() })),
        };
        let mut buf = Vec::new();
        ack_frame.encode(&mut buf).unwrap();
        ws_stream.send(Message::Binary(buf.into())).await.unwrap();
    }

    tokio::time::sleep(tokio::time::Duration::from_millis(200)).await;
    let count: i64 = sqlx::query("SELECT COUNT(*) FROM messages WHERE recipient_id = $1")
        .bind(user_b_id)
        .fetch_one(&pool)
        .await
        .unwrap()
        .get(0);
    assert_eq!(count, 3, "Messages should still be in DB before batch flush");

    tokio::time::sleep(tokio::time::Duration::from_millis(1000)).await;

    let count: i64 = sqlx::query("SELECT COUNT(*) FROM messages WHERE recipient_id = $1")
        .bind(user_b_id)
        .fetch_one(&pool)
        .await
        .unwrap()
        .get(0);
    assert_eq!(count, 0, "Messages should have been flushed from DB after interval");

    for i in 0..5 {
        send_message(&client, &server_url, &token_a, &user_b_id.to_string(), format!("batch msg {}", i).as_bytes()).await;
    }

    for _ in 0..5 {
        if let Some(Ok(Message::Binary(bin))) = ws_stream.next().await {
            let frame = WebSocketFrame::decode(bin.as_ref()).unwrap();
            if let Some(Payload::Envelope(env)) = frame.payload {
                let ack_frame = WebSocketFrame {
                    payload: Some(Payload::Ack(AckMessage { message_id: env.id })),
                };
                let mut buf = Vec::new();
                ack_frame.encode(&mut buf).unwrap();
                ws_stream.send(Message::Binary(buf.into())).await.unwrap();
            }
        }
    }

    tokio::time::sleep(tokio::time::Duration::from_millis(200)).await;
    let count: i64 = sqlx::query("SELECT COUNT(*) FROM messages WHERE recipient_id = $1")
        .bind(user_b_id)
        .fetch_one(&pool)
        .await
        .unwrap()
        .get(0);
    assert_eq!(count, 0, "Messages should have been flushed immediately when batch size hit");
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
    assert_eq!(resp.status(), 201, "User registration failed in batching test");
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
    assert_eq!(resp.status(), 201, "Message sending failed in batching test");
}

fn decode_jwt_claims(token: &str) -> serde_json::Value {
    let parts: Vec<&str> = token.split('.').collect();
    let payload = parts[1];
    let decoded = base64::engine::general_purpose::URL_SAFE_NO_PAD.decode(payload).unwrap();
    serde_json::from_slice(&decoded).unwrap()
}
