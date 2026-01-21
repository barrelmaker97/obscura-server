use obscura_server::proto::obscura::v1::EncryptedMessage;
use prost::Message as ProstMessage;
use uuid::Uuid;

mod common;

#[tokio::test]
async fn test_messaging_flow() {
    // 1. Setup
    let app = common::TestApp::spawn().await;
    let run_id = Uuid::new_v4().to_string()[..8].to_string();

    // 2. Register Users
    let user_a = app.register_user(&format!("alice_{}", run_id)).await;
    let user_b = app.register_user(&format!("bob_{}", run_id)).await;

    // 3. Send Message A -> B
    let content = b"Hello World".to_vec();
    app.send_message(&user_a.token, user_b.user_id, &content).await;

    // 4. Connect User B via WebSocket and Receive
    let mut ws = app.connect_ws(&user_b.token).await;

    let env = ws.receive_envelope().await.expect("Did not receive expected message");
    let received_msg = env.message.expect("Envelope missing message");
    assert_eq!(received_msg.content, content);
    assert_eq!(received_msg.r#type, 2);

    // 5. Send ACK
    ws.send_ack(env.id).await;
}

#[tokio::test]
async fn test_send_message_recipient_not_found() {
    let app = common::TestApp::spawn().await;
    let run_id = Uuid::new_v4().to_string()[..8].to_string();
    let user_a = app.register_user(&format!("alice_{}", run_id)).await;
    let bad_id = Uuid::new_v4();

    // Custom send to verify 404 (TestApp helper asserts 201, so we do manual here)
    let enc_msg = EncryptedMessage { r#type: 2, content: b"Hello".to_vec() };
    let mut buf = Vec::new();
    enc_msg.encode(&mut buf).unwrap();

    let resp = app
        .client
        .post(format!("{}/v1/messages/{}", app.server_url, bad_id))
        .header("Authorization", format!("Bearer {}", user_a.token))
        .header("Content-Type", "application/octet-stream")
        .body(buf)
        .send()
        .await
        .unwrap();

    assert_eq!(resp.status(), 404);
}

#[tokio::test]
async fn test_websocket_auth_failure() {
    let app = common::TestApp::spawn().await;
    let res = tokio_tungstenite::connect_async(format!("{}?token=invalid_token", app.ws_url)).await;
    assert!(res.is_err());
}

#[tokio::test]
async fn test_no_duplicate_delivery() {
    let app = common::TestApp::spawn().await;
    let run_id = Uuid::new_v4().to_string()[..8].to_string();

    let user_a = app.register_user(&format!("alice_{}", run_id)).await;
    let user_b = app.register_user(&format!("bob_{}", run_id)).await;

    // Send Msg 1
    app.send_message(&user_a.token, user_b.user_id, b"Message 1").await;

    // Connect & Receive Msg 1
    let mut ws = app.connect_ws(&user_b.token).await;
    let env1 = ws.receive_envelope().await.expect("Msg 1 missing");

    // Send Msg 2
    app.send_message(&user_a.token, user_b.user_id, b"Message 2").await;

    // Receive Msg 2
    let env2 = ws.receive_envelope().await.expect("Msg 2 missing");
    assert_ne!(env1.id, env2.id);

    // Ensure no more messages
    assert!(ws.receive_envelope_timeout(std::time::Duration::from_millis(500)).await.is_none());
}

#[tokio::test]
async fn test_redelivery_on_reconnect() {
    let app = common::TestApp::spawn().await;
    let run_id = Uuid::new_v4().to_string()[..8].to_string();

    let user_a = app.register_user(&format!("alice_{}", run_id)).await;
    let user_b = app.register_user(&format!("bob_{}", run_id)).await;

    app.send_message(&user_a.token, user_b.user_id, b"Persistent Message").await;

    // Connect, receive, but NO ACK, then disconnect
    {
        let mut ws = app.connect_ws(&user_b.token).await;
        ws.receive_envelope().await.expect("Should receive msg");
    } // Drops connection

    // Reconnect
    let mut ws = app.connect_ws(&user_b.token).await;
    let env = ws.receive_envelope().await.expect("Should receive msg again");

    // ACK this time
    ws.send_ack(env.id).await;

    // Wait for server to process ACK before disconnecting
    tokio::time::sleep(tokio::time::Duration::from_millis(1000)).await;

    // Reconnect again -> Should be empty
    drop(ws);
    let mut ws = app.connect_ws(&user_b.token).await;
    assert!(ws.receive_envelope_timeout(std::time::Duration::from_millis(500)).await.is_none());
}
