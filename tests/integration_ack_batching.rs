use futures::SinkExt;
use obscura_server::proto::obscura::v1::{AckMessage, WebSocketFrame, web_socket_frame::Payload};
use prost::Message as ProstMessage;
use sqlx::Row;
use tokio_tungstenite::tungstenite::protocol::Message;
use uuid::Uuid;

mod common;

#[tokio::test]
async fn test_ack_batching_behavior() {
    // 1. Setup with custom config
    let mut config = common::get_test_config();
    config.websocket.ack_batch_size = 5;
    config.websocket.ack_flush_interval_ms = 1000;

    let app = common::TestApp::spawn_with_config(config).await;
    let run_id = Uuid::new_v4().to_string()[..8].to_string();

    // 2. Register Users
    let user_a = app.register_user(&format!("alice_{}", run_id)).await;
    let user_b = app.register_user(&format!("bob_{}", run_id)).await;

    // 3. Send 3 messages
    for i in 0..3 {
        app.send_message(&user_a.token, user_b.user_id, format!("msg {}", i).as_bytes()).await;
    }

    // 4. Connect WS
    let mut ws = app.connect_ws(&user_b.token).await;

    // 5. Receive messages
    let mut message_ids = Vec::new();
    for _ in 0..3 {
        if let Some(env) = ws.receive_envelope().await {
            message_ids.push(env.id);
        }
    }
    assert_eq!(message_ids.len(), 3);

    // 6. Send ACKs (should be buffered)
    for id in &message_ids {
        ws.send_ack(id.clone()).await;
    }

    // 7. Verify NOT deleted immediately (Buffer < Batch Size)
    tokio::time::sleep(tokio::time::Duration::from_millis(200)).await;
    let count: i64 = sqlx::query("SELECT COUNT(*) FROM messages WHERE recipient_id = $1")
        .bind(user_b.user_id)
        .fetch_one(&app.pool)
        .await
        .unwrap()
        .get(0);
    assert_eq!(count, 3, "Messages should still be in DB before batch flush");

    // 8. Verify flushed after interval
    tokio::time::sleep(tokio::time::Duration::from_millis(1000)).await;
    let count: i64 = sqlx::query("SELECT COUNT(*) FROM messages WHERE recipient_id = $1")
        .bind(user_b.user_id)
        .fetch_one(&app.pool)
        .await
        .unwrap()
        .get(0);
    assert_eq!(count, 0, "Messages should have been flushed from DB after interval");

    // 9. Test Batch Size Trigger
    for i in 0..5 {
        app.send_message(&user_a.token, user_b.user_id, format!("batch msg {}", i).as_bytes()).await;
    }

    for _ in 0..5 {
        if let Some(env) = ws.receive_envelope().await {
            let ack_frame = WebSocketFrame { payload: Some(Payload::Ack(AckMessage { message_id: env.id })) };
            let mut buf = Vec::new();
            ack_frame.encode(&mut buf).unwrap();
            ws.stream.send(Message::Binary(buf.into())).await.unwrap();
        }
    }

    // Verify immediate flush (Buffer >= Batch Size)
    tokio::time::sleep(tokio::time::Duration::from_millis(200)).await;
    let count: i64 = sqlx::query("SELECT COUNT(*) FROM messages WHERE recipient_id = $1")
        .bind(user_b.user_id)
        .fetch_one(&app.pool)
        .await
        .unwrap()
        .get(0);
    assert_eq!(count, 0, "Messages should have been flushed immediately when batch size hit");
}
