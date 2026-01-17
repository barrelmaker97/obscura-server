use base64::Engine;
use futures::{SinkExt, StreamExt};
use obscura_server::{
    api::app_router,
    core::notification::InMemoryNotifier,
    proto::obscura::v1::{AckMessage, WebSocketFrame, web_socket_frame::Payload},
};
use prost::Message as ProstMessage;
use serde_json::json;
use std::sync::Arc;
use tokio::net::TcpListener;
use tokio_tungstenite::{connect_async, tungstenite::protocol::Message};
use uuid::Uuid;

mod common;

#[tokio::test]
async fn test_messaging_flow() {
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

    // 2. Register User A
    let user_a_name = format!("alice_{}", run_id);
    let token_a = register_user(&client, &server_url, &user_a_name, 1).await;

    // 3. Register User B
    let user_b_name = format!("bob_{}", run_id);
    let token_b = register_user(&client, &server_url, &user_b_name, 2).await;
    let claims_b = decode_jwt_claims(&token_b);
    let user_b_id = claims_b["sub"].as_str().unwrap();

    // 4. Send Message from A to B (Raw bytes)
    let content = b"Hello World".to_vec();

    let resp_msg = client
        .post(format!("{}/v1/messages/{}", server_url, user_b_id))
        .header("Authorization", format!("Bearer {}", token_a))
        .header("Content-Type", "application/octet-stream")
        .body(content.clone())
        .send()
        .await
        .unwrap();

    assert_eq!(resp_msg.status(), 201, "Message sending failed");

    // 5. Connect User B via WebSocket and Receive
    let (mut ws_stream, _) =
        connect_async(format!("{}?token={}", ws_url, token_b)).await.expect("Failed to connect WS");

    // We expect the message immediately
    if let Some(msg) = ws_stream.next().await {
        let msg = msg.unwrap();
        if let Message::Binary(bin) = msg {
            let frame = WebSocketFrame::decode(bin.as_ref()).unwrap();
            if let Some(Payload::Envelope(env)) = frame.payload {
                assert_eq!(env.content, content);

                // Send ACK
                let ack = AckMessage { message_id: env.id.clone() };
                let ack_frame = WebSocketFrame { payload: Some(Payload::Ack(ack)) };
                let mut ack_buf = Vec::new();
                ack_frame.encode(&mut ack_buf).unwrap();

                ws_stream.send(Message::Binary(ack_buf.into())).await.expect("Failed to send ACK");

                tokio::time::sleep(tokio::time::Duration::from_millis(100)).await;
                return;
            }
        }
    }

    panic!("Did not receive expected message");
}

#[tokio::test]
async fn test_send_message_recipient_not_found() {
    let pool = common::get_test_pool().await;
    let config = common::get_test_config();
    let notifier = Arc::new(InMemoryNotifier::new(config.clone()));
    let app = app_router(pool, config.clone(), notifier);

    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();
    let server_url = format!("http://{}", addr);

    tokio::spawn(async move {
        axum::serve(listener, app.into_make_service_with_connect_info::<std::net::SocketAddr>()).await.unwrap();
    });

    let client = reqwest::Client::new();
    let run_id = Uuid::new_v4().to_string()[..8].to_string();

    let user_a_name = format!("alice_{}", run_id);
    let token_a = register_user(&client, &server_url, &user_a_name, 1).await;

    let bad_id = Uuid::new_v4();
    let content = b"Hello".to_vec();

    let resp = client
        .post(format!("{}/v1/messages/{}", server_url, bad_id))
        .header("Authorization", format!("Bearer {}", token_a))
        .header("Content-Type", "application/octet-stream")
        .body(content)
        .send()
        .await
        .unwrap();

    assert_eq!(resp.status(), 404);
}

#[tokio::test]
async fn test_websocket_auth_failure() {
    let pool = common::get_test_pool().await;
    let config = common::get_test_config();
    let notifier = Arc::new(InMemoryNotifier::new(config.clone()));
    let app = app_router(pool, config.clone(), notifier);

    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();
    let ws_url = format!("ws://{}/v1/gateway", addr);

    tokio::spawn(async move {
        axum::serve(listener, app.into_make_service_with_connect_info::<std::net::SocketAddr>()).await.unwrap();
    });

    let res = connect_async(format!("{}?token=invalid_token", ws_url)).await;
    assert!(res.is_err());
}

#[tokio::test]
async fn test_no_duplicate_delivery() {
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

    send_message(&client, &server_url, &token_a, user_b_id, b"Message 1").await;

    let (mut ws_stream, _) =
        connect_async(format!("{}?token={}", ws_url, token_b)).await.expect("Failed to connect WS");

    let msg1_id = receive_envelope(&mut ws_stream).await;

    send_message(&client, &server_url, &token_a, user_b_id, b"Message 2").await;

    let msg2_id = receive_envelope(&mut ws_stream).await;
    assert_ne!(msg1_id, msg2_id);

    tokio::select! {
        _ = tokio::time::sleep(tokio::time::Duration::from_millis(500)) => {}
        msg = ws_stream.next() => {
            if let Some(Ok(Message::Binary(bin))) = msg {
                 let frame = WebSocketFrame::decode(bin.as_ref()).unwrap();
                 if let Some(Payload::Envelope(env)) = frame.payload {
                     panic!("Duplicate message: {:?}", env.id);
                 }
            }
        }
    }
}

#[tokio::test]
async fn test_redelivery_on_reconnect() {
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

    send_message(&client, &server_url, &token_a, user_b_id, b"Persistent Message").await;

    {
        let (mut ws_stream, _) =
            connect_async(format!("{}?token={}", ws_url, token_b)).await.expect("Failed to connect WS");
        let msg_id = receive_envelope(&mut ws_stream).await;
        assert!(!msg_id.is_empty());
    }

    let (mut ws_stream, _) =
        connect_async(format!("{}?token={}", ws_url, token_b)).await.expect("Failed to connect WS again");

    let msg_id_again = receive_envelope(&mut ws_stream).await;
    assert!(!msg_id_again.is_empty());

    let ack = AckMessage { message_id: msg_id_again.clone() };
    let ack_frame = WebSocketFrame { payload: Some(Payload::Ack(ack)) };
    let mut ack_buf = Vec::new();
    ack_frame.encode(&mut ack_buf).unwrap();
    ws_stream.send(Message::Binary(ack_buf.into())).await.expect("Failed to send ACK");

    tokio::time::sleep(tokio::time::Duration::from_millis(1000)).await;

    drop(ws_stream);
    let (mut ws_stream, _) =
        connect_async(format!("{}?token={}", ws_url, token_b)).await.expect("Failed to connect WS third time");

    tokio::select! {
        _ = tokio::time::sleep(tokio::time::Duration::from_millis(500)) => {}
        msg = ws_stream.next() => {
            if let Some(Ok(Message::Binary(bin))) = msg {
                 let frame = WebSocketFrame::decode(bin.as_ref()).unwrap();
                 if let Some(Payload::Envelope(env)) = frame.payload {
                     panic!("Received message after ACK: {:?}", env.id);
                 }
            }
        }
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
    assert_eq!(resp.status(), 201);
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
    assert_eq!(resp.status(), 201, "Failed to send message via helper");
}

async fn receive_envelope(
    ws_stream: &mut tokio_tungstenite::WebSocketStream<tokio_tungstenite::MaybeTlsStream<tokio::net::TcpStream>>,
) -> String {
    if let Some(Ok(Message::Binary(bin))) = ws_stream.next().await {
        let frame = WebSocketFrame::decode(bin.as_ref()).unwrap();
        if let Some(Payload::Envelope(env)) = frame.payload {
            return env.id;
        }
    }
    panic!("Failed to receive envelope");
}

fn decode_jwt_claims(token: &str) -> serde_json::Value {
    let parts: Vec<&str> = token.split('.').collect();
    let payload = parts[1];
    let decoded = base64::engine::general_purpose::URL_SAFE_NO_PAD.decode(payload).unwrap();
    serde_json::from_slice(&decoded).unwrap()
}