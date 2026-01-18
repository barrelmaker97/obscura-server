use uuid::Uuid;

mod common;

/// Verifies that the server correctly recovers from a "Lagged" notification state.
#[tokio::test]
async fn test_notification_lag_recovery() {
    // 1. Setup with small buffer
    let mut config = common::get_test_config();
    config.ws_outbound_buffer_size = 10;
    
    let app = common::TestApp::spawn_with_config(config).await;
    let run_id = Uuid::new_v4().to_string()[..8].to_string();

    // 2. Register Users
    let (token_a, _) = app.register_user(&format!("alice_{}", run_id)).await;
    let (token_b, user_b_id) = app.register_user(&format!("bob_{}", run_id)).await;

    // 3. Connect User B but DO NOT read from the stream initially
    let mut ws = app.connect_ws(&token_b).await;

    // 4. Flood the system with messages (100 > 10 buffer)
    let message_count = 100;
    for i in 0..message_count {
        let content = format!("Message {}", i).into_bytes();
        app.send_message(&token_a, user_b_id, &content).await;
    }

    // Allow time for notifications to propagate and overflow
    tokio::time::sleep(tokio::time::Duration::from_millis(500)).await;

    // 5. Start reading everything from the WebSocket
    let mut received = 0;
    // We expect 100 messages. TestWsClient helper waits up to 5s for *one* message.
    // We'll just loop until we get them all or timeout.
    // However, the helper `receive_envelope_timeout` is useful here.
    
    // We need a tighter loop than the default 5s per message for bulk reading
    let start = std::time::Instant::now();
    let timeout = std::time::Duration::from_secs(10);
    
    while received < message_count && start.elapsed() < timeout {
        if let Some(_) = ws.receive_envelope_timeout(std::time::Duration::from_millis(100)).await {
            received += 1;
        }
    }

    // Verify that the server's lag-recovery logic successfully delivered ALL messages
    assert_eq!(received, message_count, "Should receive all {} messages despite notification lag", message_count);
}
