mod common;

use obscura_server::proto::obscura::v1::{EncryptedMessage, WebSocketFrame, web_socket_frame::Payload, AckMessage};
use prost::Message as ProstMessage;
use uuid::Uuid;
use common::TestApp;
use futures::SinkExt;
use std::time::Duration;
use tokio_tungstenite::tungstenite::protocol::Message as WsMessage;

#[tokio::test]
async fn test_messaging_flow() {
    let app = TestApp::spawn().await;
    let run_id = Uuid::new_v4().to_string()[..8].to_string();

    let user_a = app.register_user(&format!("alice_{}", run_id)).await;
    let user_b = app.register_user(&format!("bob_{}", run_id)).await;

    let content = b"Hello World".to_vec();
    app.send_message(&user_a.token, user_b.user_id, &content).await;

    let mut ws = app.connect_ws(&user_b.token).await;
    let env = ws.receive_envelope().await.expect("Did not receive message");
    let received_msg = env.message.expect("Envelope missing message");
    assert_eq!(received_msg.content, content);
    
    ws.send_ack(env.id).await;
}

#[tokio::test]
async fn test_message_pagination_large_backlog() {
    let app = TestApp::spawn().await;
    let run_id = Uuid::new_v4().to_string()[..8].to_string();

    let user_a = app.register_user(&format!("alice_{}", run_id)).await;
    let user_b = app.register_user(&format!("bob_{}", run_id)).await;

    let message_count = 125;
    for i in 0..message_count {
        let content = format!("Message {}", i);
        app.send_message(&user_a.token, user_b.user_id, content.as_bytes()).await;
    }

    let mut ws = app.connect_ws(&user_b.token).await;

    let mut received_ids = Vec::new();
    let timeout = Duration::from_secs(10);
    let start = std::time::Instant::now();

    while received_ids.len() < message_count && start.elapsed() < timeout {
        if let Some(env) = ws.receive_envelope_timeout(Duration::from_millis(1000)).await {
            if !received_ids.contains(&env.id) {
                received_ids.push(env.id);
            }
        } else {
            break;
        }
    }

    assert_eq!(received_ids.len(), message_count, "Did not receive all messages");
}

#[tokio::test]
async fn test_ack_batching_behavior() {
    let mut config = common::get_test_config();
    config.websocket.ack_batch_size = 5;
    config.websocket.ack_flush_interval_ms = 1000;

    let app = TestApp::spawn_with_config(config).await;
    let run_id = Uuid::new_v4().to_string()[..8].to_string();

    let user_a = app.register_user(&format!("alice_{}", run_id)).await;
    let user_b = app.register_user(&format!("bob_{}", run_id)).await;

    for i in 0..3 {
        app.send_message(&user_a.token, user_b.user_id, format!("msg {}", i).as_bytes()).await;
    }

    let mut ws = app.connect_ws(&user_b.token).await;

    let mut message_ids = Vec::new();
    for _ in 0..3 {
        if let Some(env) = ws.receive_envelope().await {
            message_ids.push(env.id);
        }
    }
    assert_eq!(message_ids.len(), 3);

    for id in &message_ids {
        ws.send_ack(id.clone()).await;
    }

    // Verify NOT deleted immediately (Buffer < Batch Size)
    tokio::time::sleep(Duration::from_millis(200)).await;
    app.assert_message_count(user_b.user_id, 3).await;

    // Verify flushed after interval
    app.assert_message_count(user_b.user_id, 0).await;
}

#[tokio::test]
async fn test_send_message_recipient_not_found() {
    let app = TestApp::spawn().await;
    let run_id = Uuid::new_v4().to_string()[..8].to_string();
    let user_a = app.register_user(&format!("alice_404_{}", run_id)).await;
    let bad_id = Uuid::new_v4();

    let enc_msg = EncryptedMessage { r#type: 2, content: b"Hello".to_vec() };
    let mut buf = Vec::new();
    enc_msg.encode(&mut buf).unwrap();

    let resp = app.client
        .post(format!("{}/v1/messages/{}", app.server_url, bad_id))
        .header("Authorization", format!("Bearer {}", user_a.token))
        .header("Content-Type", "application/octet-stream")
        .body(buf).send().await.unwrap();

    assert_eq!(resp.status(), 404);
}

#[tokio::test]
async fn test_send_message_malformed_protobuf() {
    let app = TestApp::spawn().await;
    let run_id = Uuid::new_v4().to_string()[..8].to_string();
    let user = app.register_user(&format!("malformed_{}", run_id)).await;
    let recipient = app.register_user(&format!("recipient_malformed_{}", run_id)).await;

    let resp = app.client
        .post(format!("{}/v1/messages/{}", app.server_url, recipient.user_id))
        .header("Authorization", format!("Bearer {}", user.token))
        .header("Content-Type", "application/octet-stream")
        .body(vec![0, 1, 2]).send().await.unwrap();

    assert_eq!(resp.status(), 400);
}

#[tokio::test]
async fn test_websocket_auth_failure() {
    let app = TestApp::spawn().await;
    let res = tokio_tungstenite::connect_async(format!("{}?token=invalid", app.ws_url)).await;
    assert!(res.is_err());
}

#[tokio::test]
async fn test_gateway_missing_identity_key() {
    let app = TestApp::spawn().await;
    let run_id = Uuid::new_v4().to_string()[..8].to_string();
    let user = app.register_user(&format!("missing_id_{}", run_id)).await;

    sqlx::query("DELETE FROM identity_keys WHERE user_id = $1").bind(user.user_id).execute(&app.pool).await.unwrap();

    let url = format!("{}?token={}", app.ws_url, user.token);
    let res = tokio_tungstenite::connect_async(url).await;

    match res {
        Ok((mut socket, _)) => {
            // Wait for close
            let mut closed = false;
            let start = std::time::Instant::now();
            while start.elapsed() < Duration::from_secs(5) {
                use futures::StreamExt;
                match socket.next().await {
                    Some(Ok(WsMessage::Close(_))) | None => {
                        closed = true;
                        break;
                    }
                    Some(Err(_)) => {
                        closed = true;
                        break;
                    }
                    _ => {}
                }
            }
            assert!(closed, "Socket should have been closed by server");
        }
        Err(_) => {
            // If handshake failed, that's also valid closure
        }
    }
}

#[tokio::test]
async fn test_ack_buffer_saturation() {
    let mut config = common::get_test_config();
    config.websocket.ack_buffer_size = 5;
    config.websocket.ack_batch_size = 1;

    let app = TestApp::spawn_with_config(config).await;
    let run_id = Uuid::new_v4().to_string()[..8].to_string();
    let user = app.register_user(&format!("ack_sat_{}", run_id)).await;
    let mut client = app.connect_ws(&user.token).await;

    for _ in 0..15 {
        let ack = AckMessage { message_id: Uuid::new_v4().to_string() };
        let frame = WebSocketFrame { payload: Some(Payload::Ack(ack)) };
        let mut buf = Vec::new();
        frame.encode(&mut buf).unwrap();
        client.sink.send(WsMessage::Binary(buf.into())).await.unwrap();
    }

    client.sink.send(WsMessage::Ping(vec![].into())).await.unwrap();
    assert!(client.receive_pong().await.is_some());
}
