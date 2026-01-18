use tokio_tungstenite::tungstenite::protocol::Message;
use futures::{SinkExt, StreamExt};
use uuid::Uuid;

mod common;

#[tokio::test]
async fn test_ping_pong_under_load() {
    // 1. Setup with large batch limit
    let mut config = common::get_test_config();
    config.message_batch_limit = 100;

    let app = common::TestApp::spawn_with_config(config).await;
    let run_id = Uuid::new_v4().to_string()[..8].to_string();

    // 2. Register Users
    let (token_a, _) = app.register_user(&format!("alice_{}", run_id)).await;
    let (token_b, user_b_id) = app.register_user(&format!("bob_{}", run_id)).await;

    // 3. Fill Inbox with LARGE messages to fill TCP buffer
    // 100 messages * 500KB = 50MB.
    let large_payload = vec![0u8; 1024 * 500];
    for _ in 0..100 {
        app.send_message(&token_a, user_b_id, &large_payload).await;
    }

    // 4. Connect via WebSocket
    let mut ws = app.connect_ws(&token_b).await;

    // 5. Confirm server has started flushing (receive at least one binary)
    // We access the raw stream here because we are testing protocol level ping/pong
    match ws.stream.next().await {
        Some(Ok(Message::Binary(_))) => {}
        _ => panic!("Expected initial binary message"),
    }

    // 6. Send a PING.
    ws.stream.send(Message::Ping(vec![1, 2, 3].into())).await.unwrap();

    // 7. Verify the Pong arrives QUICKLY (before the whole batch finishes)
    let mut binary_count = 0;
    let mut pong_received = false;
    let timeout = tokio::time::Duration::from_secs(5);
    let start = std::time::Instant::now();

    while start.elapsed() < timeout {
        match tokio::time::timeout(tokio::time::Duration::from_millis(500), ws.stream.next()).await {
            Ok(Some(Ok(msg))) => {
                match msg {
                    Message::Pong(payload) => {
                        assert_eq!(payload, vec![1, 2, 3]);
                        pong_received = true;
                        break;
                    }
                    Message::Binary(_) => {
                        binary_count += 1;
                    }
                    _ => {}
                }
            }
            _ => break,
        }
    }

    assert!(pong_received, "Did not receive Pong under load");
    assert!(binary_count < 40, "Server blocked! Received {} binary messages before Pong", binary_count);
}