mod common;

use common::TestApp;
use futures::SinkExt;
use obscura_server::proto::obscura::v1::{
    web_socket_frame::Payload, send_message_response, AckMessage, EncryptedMessage, SendMessageResponse, WebSocketFrame,
};
use prost::Message as ProstMessage;
use std::time::Duration;
use tokio_tungstenite::tungstenite::protocol::Message as WsMessage;
use uuid::Uuid;

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

    let user_a = app.register_user(&format!("alice_pag_{}", run_id)).await;
    let user_b = app.register_user(&format!("bob_pag_{}", run_id)).await;

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

    let user_a = app.register_user(&format!("alice_ack_{}", run_id)).await;
    let user_b = app.register_user(&format!("bob_ack_{}", run_id)).await;

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

    let client_msg_id = Uuid::new_v4().to_string();
    let request = obscura_server::proto::obscura::v1::SendMessageRequest {
        messages: vec![obscura_server::proto::obscura::v1::OutgoingMessage {
            client_message_id: client_msg_id.clone(),
            recipient_id: bad_id.to_string(),
            client_timestamp_ms: 123456,
            message: Some(EncryptedMessage { r#type: 2, content: b"Hello".to_vec() }),
        }],
    };
    let mut buf = Vec::new();
    request.encode(&mut buf).unwrap();

    let resp = app
        .client
        .post(format!("{}/v1/messages", app.server_url))
        .header("Authorization", format!("Bearer {}", user_a.token))
        .header("Content-Type", "application/x-protobuf")
        .body(buf)
        .send()
        .await
        .unwrap();

    assert_eq!(resp.status(), 200);
    let body = resp.bytes().await.unwrap();
    let response = SendMessageResponse::decode(body).unwrap();
    assert_eq!(response.failed_messages.len(), 1);
    assert_eq!(response.failed_messages[0].client_message_id, client_msg_id);
    assert_eq!(
        response.failed_messages[0].error_code,
        send_message_response::ErrorCode::UserNotFound as i32
    );
}

#[tokio::test]
async fn test_send_message_malformed_protobuf() {
    let app = TestApp::spawn().await;
    let run_id = Uuid::new_v4().to_string()[..8].to_string();
    let user = app.register_user(&format!("malformed_{}", run_id)).await;

    let resp = app
        .client
        .post(format!("{}/v1/messages", app.server_url))
        .header("Authorization", format!("Bearer {}", user.token))
        .header("Content-Type", "application/x-protobuf")
        .body(vec![0, 1, 2])
        .send()
        .await
        .unwrap();

    assert_eq!(resp.status(), 400);
}

#[tokio::test]
async fn test_message_idempotency() {
    let app = TestApp::spawn().await;
    let run_id = Uuid::new_v4().to_string()[..8].to_string();
    let user_a = app.register_user(&format!("alice_idem_{}", run_id)).await;
    let user_b = app.register_user(&format!("bob_idem_{}", run_id)).await;

    let idempotency_key = Uuid::new_v4();
    let content = b"Idempotent Hello".to_vec();

    let request = obscura_server::proto::obscura::v1::SendMessageRequest {
        messages: vec![obscura_server::proto::obscura::v1::OutgoingMessage {
            client_message_id: Uuid::new_v4().to_string(),
            recipient_id: user_b.user_id.to_string(),
            client_timestamp_ms: 123456789,
            message: Some(EncryptedMessage { r#type: 2, content: content.clone() }),
        }],
    };
    let mut buf = Vec::new();
    request.encode(&mut buf).unwrap();

    // First attempt
    let resp1 = app
        .client
        .post(format!("{}/v1/messages", app.server_url))
        .header("Authorization", format!("Bearer {}", user_a.token))
        .header("Idempotency-Key", idempotency_key.to_string())
        .header("Content-Type", "application/x-protobuf")
        .body(buf.clone())
        .send()
        .await
        .unwrap();

    assert_eq!(resp1.status(), 200);
    let body1 = resp1.bytes().await.unwrap();

    // Second attempt (retry)
    let resp2 = app
        .client
        .post(format!("{}/v1/messages", app.server_url))
        .header("Authorization", format!("Bearer {}", user_a.token))
        .header("Idempotency-Key", idempotency_key.to_string())
        .header("Content-Type", "application/x-protobuf")
        .body(buf)
        .send()
        .await
        .unwrap();

    assert_eq!(resp2.status(), 200);
    let body2 = resp2.bytes().await.unwrap();

    // Verify bodies are identical (cached response)
    assert_eq!(body1, body2);

    // Verify only ONE message was queued
    app.assert_message_count(user_b.user_id, 1).await;
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
    let user = app.register_user(&format!("miss_id_{}", run_id)).await;

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
        let ack = AckMessage { message_id: Uuid::new_v4().to_string(), message_ids: vec![] };
        let frame = WebSocketFrame { payload: Some(Payload::Ack(ack)) };
        let mut buf = Vec::new();
        frame.encode(&mut buf).unwrap();
        client.sink.send(WsMessage::Binary(buf.into())).await.unwrap();
    }

    client.sink.send(WsMessage::Ping(vec![].into())).await.unwrap();
    assert!(client.receive_pong().await.is_some());
}

#[tokio::test]
async fn test_bulk_ack_processing() {
    let app = TestApp::spawn().await;
    let run_id = Uuid::new_v4().to_string()[..8].to_string();

    let user_a = app.register_user(&format!("alice_bulk_{}", run_id)).await;
    let user_b = app.register_user(&format!("bob_bulk_{}", run_id)).await;

    // Send 5 messages
    for i in 0..5 {
        app.send_message(&user_a.token, user_b.user_id, format!("msg {}", i).as_bytes()).await;
    }

    let mut ws = app.connect_ws(&user_b.token).await;

    // Receive all 5
    let mut message_ids = Vec::new();
    for _ in 0..5 {
        if let Some(env) = ws.receive_envelope().await {
            message_ids.push(env.id);
        }
    }
    assert_eq!(message_ids.len(), 5);

    // Send ONE bulk ACK
    let ack = AckMessage {
        message_id: String::new(), // Legacy field empty
        message_ids: message_ids.clone(),
    };
    let frame = WebSocketFrame { payload: Some(Payload::Ack(ack)) };
    let mut buf = Vec::new();
    frame.encode(&mut buf).unwrap();
    ws.sink.send(WsMessage::Binary(buf.into())).await.unwrap();

    // Verify all 5 are deleted
    app.assert_message_count(user_b.user_id, 0).await;
}

#[tokio::test]
async fn test_mixed_ack_processing() {
    let app = TestApp::spawn().await;
    let run_id = Uuid::new_v4().to_string()[..8].to_string();

    let user_a = app.register_user(&format!("alice_mix_{}", run_id)).await;
    let user_b = app.register_user(&format!("bob_mix_{}", run_id)).await;

    // Send 3 messages
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

    // Send ACK with BOTH fields
    // message_id = First message
    // message_ids = Remaining two messages
    let ack = AckMessage {
        message_id: message_ids[0].clone(),
        message_ids: vec![message_ids[1].clone(), message_ids[2].clone()],
    };
    let frame = WebSocketFrame { payload: Some(Payload::Ack(ack)) };
    let mut buf = Vec::new();
    frame.encode(&mut buf).unwrap();
    ws.sink.send(WsMessage::Binary(buf.into())).await.unwrap();

    // Verify all 3 are deleted
    app.assert_message_count(user_b.user_id, 0).await;
}

#[tokio::test]
async fn test_ack_security_cross_user_deletion() {
    let app = TestApp::spawn().await;
    let run_id = Uuid::new_v4().to_string()[..8].to_string();

    let user_a = app.register_user(&format!("alice_sec_{}", run_id)).await;
    let user_b = app.register_user(&format!("bob_sec_{}", run_id)).await;
    let user_c = app.register_user(&format!("eve_sec_{}", run_id)).await;

    // A sends to B
    app.send_message(&user_a.token, user_b.user_id, b"Secret").await;

    // B connects and gets the message ID
    let mut ws_b = app.connect_ws(&user_b.token).await;
    let env = ws_b.receive_envelope().await.expect("Bob should receive message");
    let target_msg_id = env.id;

    // C tries to ACK B's message
    let mut ws_c = app.connect_ws(&user_c.token).await;
    ws_c.send_ack(target_msg_id.clone()).await;

    // Wait for async processing
    tokio::time::sleep(Duration::from_millis(500)).await;

    // Assert message is STILL there for B (count should be 1)
    app.assert_message_count(user_b.user_id, 1).await;
}
