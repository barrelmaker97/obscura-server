use uuid::Uuid;

mod common;

#[tokio::test]
async fn test_message_pagination_large_backlog() {
    let app = common::TestApp::spawn().await;
    let run_id = Uuid::new_v4().to_string()[..8].to_string();

    let (token_a, _) = app.register_user(&format!("alice_{}", run_id)).await;
    let (token_b, user_b_id) = app.register_user(&format!("bob_{}", run_id)).await;

    let message_count = 125;
    for i in 0..message_count {
        let content = format!("Message {}", i);
        app.send_message(&token_a, user_b_id, content.as_bytes()).await;
    }

    let mut ws = app.connect_ws(&token_b).await;

    let mut received_messages = Vec::new();
    let mut received_ids = Vec::new();
    let timeout = std::time::Duration::from_secs(10);
    let start = std::time::Instant::now();

    while received_messages.len() < message_count && start.elapsed() < timeout {
        // Use a 1s timeout per read, similar to original logic
        if let Some(env) = ws.receive_envelope_timeout(std::time::Duration::from_millis(1000)).await {
            if !received_ids.contains(&env.id) {
                received_ids.push(env.id);
                let content = env.message.unwrap().content;
                received_messages.push(String::from_utf8(content).unwrap());
            }
        } else {
            break;
        }
    }

    assert_eq!(received_messages.len(), message_count, "Did not receive all messages in pagination test");

    for (i, msg) in received_messages.iter().enumerate().take(message_count) {
        assert_eq!(msg.as_bytes(), format!("Message {}", i).as_bytes(), "Message content mismatch at index {}", i);
    }
}
