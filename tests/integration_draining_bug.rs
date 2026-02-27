use reqwest::StatusCode;
use std::time::Duration;
use tokio::time::Instant;

mod common;

#[tokio::test]
async fn test_concurrent_messages_and_prekeys_no_drops() {
    // This test hammers the WebSocket with concurrent MessageReceived and PreKeyLow events.
    // If try_recv() is incorrectly draining channels, some events will be dropped,
    // and the client won't receive all 50 messages or won't receive the disconnect.
    let app = common::TestApp::spawn_with_workers(common::get_test_config()).await;

    // Alice has 100 keys (threshold is 20, but we just want to fetch them to trigger PreKeyLow)
    // Actually, PreKeyLow is only triggered if count < threshold.
    // So we give Alice 21 keys, and fetch 100 times. Every fetch below 20 triggers PreKeyLow!
    let alice = app.register_user_with_keys(&common::generate_username("alice_hammer"), 123, 21).await;
    let bob = app.register_user(&common::generate_username("bob_hammer")).await;

    let mut alice_ws = app.connect_ws(&alice.token).await;
    alice_ws.ensure_subscribed().await;

    let _ = alice_ws.receive_prekey_status_timeout(Duration::from_secs(1)).await;

    // Send 50 messages and fetch 50 keys concurrently using join!
    // We just interleave sending messages and fetching keys to trigger both
    // MessageReceived and PreKeyLow events rapidly.

    let token = bob.token.clone();
    let alice_id = alice.user_id;

    for i in 0..50 {
        app.send_message(&token, alice_id, format!("msg {}", i).as_bytes()).await;
        app.client
            .get(format!("{}/v1/keys/{}", app.server_url, alice_id))
            .header("Authorization", format!("Bearer {}", token))
            .send()
            .await
            .expect("Failed to fetch pre-key bundle");
    }

    // Now Alice must receive exactly 50 messages.
    // If the bug exists, some MessageReceived notifications will be swallowed by `try_recv`,
    // and since the message pump only runs on notification, it will stall and not fetch all 50.
    let mut received_messages = 0;

    // We give it a generous timeout to process everything.
    let start = Instant::now();
    while start.elapsed() < Duration::from_secs(5) {
        if let Some(_) = alice_ws.receive_envelope_timeout(Duration::from_millis(100)).await {
            received_messages += 1;
        }

        // Also drain statuses so the channel doesn't block
        let _ = alice_ws.receive_prekey_status_timeout(Duration::from_millis(10)).await;

        if received_messages == 50 {
            break;
        }
    }

    // Since we sent 50 messages, the client MUST receive 50 messages.
    // If it's less than 50, the `try_recv` swallowed some notifications.
    assert_eq!(
        received_messages, 50,
        "Expected 50 messages, but only received {}. Some notifications were dropped!",
        received_messages
    );
}
