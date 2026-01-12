use tokio::net::TcpListener;
use obscura_server::{api::app_router, core::notification::InMemoryNotifier};
use tokio_tungstenite::{connect_async, tungstenite::protocol::Message};
use futures::{StreamExt, SinkExt};
use serde_json::json;
use uuid::Uuid;
use obscura_server::proto::obscura::v1::{WebSocketFrame, OutgoingMessage, AckMessage, web_socket_frame::Payload};
use prost::Message as ProstMessage;
use base64::Engine;
use std::sync::Arc;

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

    let resp_a = client.post(format!("{}/v1/accounts", server_url))
        .json(&reg_a)
        .send()
        .await
        .unwrap();
    assert_eq!(resp_a.status(), 201);
    let token_a = resp_a.json::<serde_json::Value>().await.unwrap()["token"].as_str().unwrap().to_string();

    // 3. Register User B
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

    let resp_b = client.post(format!("{}/v1/accounts", server_url))
        .json(&reg_b)
        .send()
        .await
        .unwrap();
    assert_eq!(resp_b.status(), 201);

    let token_b = resp_b.json::<serde_json::Value>().await.unwrap()["token"].as_str().unwrap().to_string();
    let claims_b = decode_jwt_claims(&token_b);
    let user_b_id = claims_b["sub"].as_str().unwrap();

    // 4. Send Message from A to B
    let outgoing = OutgoingMessage {
        r#type: 1,
        content: b"Hello World".to_vec(),
        timestamp: 123456789,
    };
    let mut buf = Vec::new();
    outgoing.encode(&mut buf).unwrap();

    let resp_msg = client.post(format!("{}/v1/messages/{}", server_url, user_b_id))
        .header("Authorization", format!("Bearer {}", token_a))
        .header("Content-Type", "application/octet-stream")
        .body(buf)
        .send()
        .await
        .unwrap();

    assert_eq!(resp_msg.status(), 201);

    // 5. Connect User B via WebSocket and Receive
    let (mut ws_stream, _) = connect_async(format!("{}?token={}", ws_url, token_b))
        .await
        .expect("Failed to connect WS");

    // We expect the message immediately
    if let Some(msg) = ws_stream.next().await {
        let msg = msg.unwrap();
        if let Message::Binary(bin) = msg {
            // Using as_ref() to get &[u8]
            let frame = WebSocketFrame::decode(bin.as_ref()).unwrap();
            if let Some(Payload::Envelope(env)) = frame.payload {
                assert_eq!(env.content, b"Hello World");

                // Send ACK
                let ack = AckMessage {
                    message_id: env.id.clone(),
                };
                let ack_frame = WebSocketFrame {
                    request_id: 0,
                    payload: Some(Payload::Ack(ack)),
                };
                let mut ack_buf = Vec::new();
                ack_frame.encode(&mut ack_buf).unwrap();

                ws_stream.send(Message::Binary(ack_buf.into())).await.expect("Failed to send ACK");

                // Allow a small window for server to process ACK (optional but good for stability)
                tokio::time::sleep(tokio::time::Duration::from_millis(100)).await;
                return;
            }
        }
    }

    panic!("Did not receive expected message");
}

#[tokio::test]
async fn test_websocket_auth_failure() {
    // 1. Setup Server
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

    // Attempt to connect with invalid token
    let res = connect_async(format!("{}?token=invalid_token", ws_url)).await;

    assert!(res.is_err(), "WebSocket connection should fail with invalid token");
}

fn decode_jwt_claims(token: &str) -> serde_json::Value {
    let parts: Vec<&str> = token.split('.').collect();
    let payload = parts[1];
    let decoded = base64::engine::general_purpose::URL_SAFE_NO_PAD.decode(payload).unwrap();
    serde_json::from_slice(&decoded).unwrap()
}
