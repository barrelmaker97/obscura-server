use futures::{SinkExt, StreamExt};
use obscura_server::proto::obscura::v1::{AckMessage, WebSocketFrame, web_socket_frame::Payload};
use prost::Message;
use reqwest::StatusCode;
use tokio_tungstenite::tungstenite::protocol::Message as WsMessage;
use uuid::Uuid;

mod common;

#[tokio::test]
async fn test_send_message_malformed_protobuf() {
    let app = common::TestApp::spawn().await;
    let run_id = Uuid::new_v4().to_string()[..8].to_string();
    let username = format!("msg_fail_user_{}", run_id);
    let user = app.register_user(&username).await;
    let recipient = app.register_user(&format!("recipient_{}", run_id)).await;

    // Send random junk bytes instead of a valid EncryptedMessage protobuf
    let junk_body = vec![0u8, 1, 2, 3, 4, 255];

    let resp = app
        .client
        .post(format!("{}/v1/messages/{}", app.server_url, recipient.user_id))
        .header("Authorization", format!("Bearer {}", user.token))
        .header("Content-Type", "application/octet-stream")
        .body(junk_body)
        .send()
        .await
        .unwrap();

    // Should return 400 Bad Request
    assert_eq!(resp.status(), StatusCode::BAD_REQUEST);

    // In the server logs (if we could capture them), we would see:
    // "Failed to decode EncryptedMessage protobuf from user ...: failed to decode ..."
}

#[tokio::test]
async fn test_upload_keys_bad_signature() {
    let app = common::TestApp::spawn().await;
    let run_id = Uuid::new_v4().to_string()[..8].to_string();
    let username = format!("sig_fail_user_{}", run_id);
    let user = app.register_user(&username).await;

    // 1. Generate a valid payload first
    let (mut payload, _) = common::generate_registration_payload("temp", "pass", 1, 0);

    // 2. Extract the signed pre-key section
    let spk = payload["signedPreKey"].as_object_mut().unwrap();

    // 3. TAMPER with the signature: Just change the first character of the base64 string
    // This ensures it's still valid base64 (probably), but definitely an invalid signature.
    let mut sig_str = spk["signature"].as_str().unwrap().to_string();
    let first_char = sig_str.chars().next().unwrap();
    let new_char = if first_char == 'A' { 'B' } else { 'A' };
    sig_str.replace_range(0..1, &new_char.to_string());

    spk["signature"] = serde_json::json!(sig_str);

    // 4. Try to upload this tampered key set
    let resp = app
        .client
        .post(format!("{}/v1/keys", app.server_url))
        .header("Authorization", format!("Bearer {}", user.token))
        .json(&payload)
        .send()
        .await
        .unwrap();

    // The server should catch the signature mismatch
    assert_eq!(resp.status(), StatusCode::BAD_REQUEST);

    // In logs: "Signature verification failed for key_id ..."
}

#[tokio::test]
async fn test_websocket_malformed_frame() {
    let app = common::TestApp::spawn().await;
    let run_id = Uuid::new_v4().to_string()[..8].to_string();
    let user = app.register_user(&format!("ws_junk_user_{}", run_id)).await;

    // Connect manually so we can send raw garbage
    let url = format!("{}?token={}", app.ws_url, user.token);
    let (mut socket, _) = tokio_tungstenite::connect_async(url).await.expect("Failed to connect");

    // Send random binary garbage
    let junk = vec![0x99, 0x88, 0x77];
    socket.send(WsMessage::Binary(junk.into())).await.unwrap();

    // The server should log a warning but keep the connection open (or close it depending on policy).
    // Our code logs "Failed to decode WebSocket frame..." and continues loop.

    // Let's verify we can still send a valid Ping/Pong to ensure connection is alive
    socket.send(WsMessage::Ping(vec![].into())).await.unwrap();

    // We might receive other messages first (like PreKeyStatus), so drain until Pong
    let mut pong_received = false;
    for _ in 0..5 {
        if let Some(Ok(msg)) = socket.next().await {
            if let WsMessage::Pong(_) = msg {
                pong_received = true;
                break;
            }
        }
    }

    assert!(pong_received, "Did not receive Pong response");
}

#[tokio::test]
async fn test_websocket_invalid_ack_uuid() {
    let app = common::TestApp::spawn().await;
    let run_id = Uuid::new_v4().to_string()[..8].to_string();
    let user = app.register_user(&format!("ws_ack_fail_user_{}", run_id)).await;

    let mut client = app.connect_ws(&user.token).await;

    // Manually construct a frame with an invalid UUID string
    let ack = AckMessage { message_id: "not-a-uuid".to_string() };
    let frame = WebSocketFrame { payload: Some(Payload::Ack(ack)) };
    let mut buf = Vec::new();
    frame.encode(&mut buf).unwrap();

    // Send it
    client.sink.send(WsMessage::Binary(buf.into())).await.unwrap();

    // Verify connection is still alive (server logs warning but doesn't crash/close)
    client.sink.send(WsMessage::Ping(vec![].into())).await.unwrap();
    let pong = client.receive_pong().await;
    assert!(pong.is_some());
}

#[tokio::test]
async fn test_gateway_missing_identity_key() {
    let app = common::TestApp::spawn().await;
    let run_id = Uuid::new_v4().to_string()[..8].to_string();
    let user = app.register_user(&format!("missing_id_user_{}", run_id)).await;

    // 1. Manually DELETE the identity key from the DB to simulate this weird state
    sqlx::query("DELETE FROM identity_keys WHERE user_id = $1").bind(user.user_id).execute(&app.pool).await.unwrap();

    // 2. Try to connect to Gateway
    let url = format!("{}?token={}", app.ws_url, user.token);
    let res = tokio_tungstenite::connect_async(url).await;

    // 3. The server should have closed the connection immediately.
    // Tungstenite might return an error during handshake or an immediate Close frame.
    // In `handle_socket`, we do `socket.close().await; return;`.

    // Note: depending on timing, connect_async might succeed initially but the first read would be a Close.
    match res {
        Ok((mut socket, _)) => {
            // If it connected, expect immediate closure
            let msg = socket.next().await;
            match msg {
                Some(Ok(WsMessage::Close(_))) => {} // Correct behavior
                None => {}                          // Socket closed
                Some(Err(_)) => {}                  // Connection reset
                other => panic!("Expected connection close, got {:?}", other),
            }
        }
        Err(_) => {
            // Handshake failed or connection closed immediately, which is also valid
        }
    }
}
