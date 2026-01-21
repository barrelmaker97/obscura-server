use futures::SinkExt;
use tokio_tungstenite::tungstenite::protocol::Message;
use uuid::Uuid;

mod common;

#[tokio::test]
async fn test_ping_pong_under_load() {
    // 1. Setup with large batch limit
    let mut config = common::get_test_config();
    config.messaging.batch_limit = 100;

    let app = common::TestApp::spawn_with_config(config).await;
    let run_id = Uuid::new_v4().to_string()[..8].to_string();

    // 2. Register Users
    let user_a = app.register_user(&format!("alice_{}", run_id)).await;
    let user_b = app.register_user(&format!("bob_{}", run_id)).await;

    // 3. Fill Inbox with LARGE messages to fill TCP buffer
    // 100 messages * 500KB = 50MB.
    let large_payload = vec![0u8; 1024 * 500];
    for _ in 0..100 {
        app.send_message(&user_a.token, user_b.user_id, &large_payload).await;
    }

    // 4. Connect via WebSocket
    let mut ws = app.connect_ws(&user_b.token).await;

    // 5. Confirm server has started flushing (receive at least one binary)
    let _env = ws.receive_envelope().await.expect("Expected initial binary message");

    // 6. Send a PING manually via the sink.
    ws.sink.send(Message::Ping(vec![1, 2, 3].into())).await.unwrap();

    // 7. Verify the Pong arrives QUICKLY (before the whole batch finishes)
    let pong_payload = ws.receive_pong().await.expect("Did not receive Pong under load");
    assert_eq!(pong_payload, vec![1, 2, 3]);
}
