use reqwest::StatusCode;
use std::time::Duration;

mod common;

#[tokio::test]
async fn test_prekey_signaling_realtime() {
    let app = common::TestApp::spawn_with_workers(common::get_test_config()).await;

    // 1. Register Bob with exactly 21 OTPKs (threshold is 20)
    let bob = app.register_user_with_keys(&common::generate_username("bob"), 123, 21).await;

    // 2. Bob connects to Gateway
    let mut bob_ws = app.connect_ws(&bob.token).await;
    bob_ws.ensure_subscribed().await;

    // 3. Alice fetches Bob's pre-key bundle
    // This should consume 1 key, leaving 20. Threshold is 20,
    // so < 20 check in service will NOT trigger yet if check is 'count < threshold'.
    // If threshold is 20, then 19 triggers it.
    let alice = app.register_user(&common::generate_username("alice")).await;

    let resp = app
        .client
        .get(format!("{}/v1/keys/{}", app.server_url, bob.user_id))
        .header("Authorization", format!("Bearer {}", alice.token))
        .send()
        .await
        .expect("Failed to fetch pre-key bundle");
    assert_eq!(resp.status(), StatusCode::OK);

    // 4. Alice fetches Bob's bundle AGAIN.
    // Now count becomes 19. This should trigger the notification.
    let resp = app
        .client
        .get(format!("{}/v1/keys/{}", app.server_url, bob.user_id))
        .header("Authorization", format!("Bearer {}", alice.token))
        .send()
        .await
        .expect("Failed to fetch pre-key bundle second time");
    assert_eq!(resp.status(), StatusCode::OK);

    // 5. Check Bob's WS for PreKeyStatus
    let status = bob_ws.receive_prekey_status_timeout(Duration::from_secs(2)).await;
    assert!(status.is_some(), "Bob should have received a PreKeyStatus frame");
    let status = status.expect("PreKeyStatus frame missing");
    assert_eq!(status.one_time_pre_key_count, 19);
    assert_eq!(status.min_threshold, 20);
}

#[tokio::test]
async fn test_prekey_signaling_push() {
    let mut config = common::get_test_config();
    // Set a very low push delay for testing
    config.notifications.push_delay_secs = 0;
    let app = common::TestApp::spawn_with_workers(config).await;

    // 1. Register Bob with 21 keys, but NO WebSocket connection
    let bob = app.register_user_with_keys(&common::generate_username("bob_offline"), 123, 21).await;

    // Set a mock push token so Bob is eligible for pushes
    app.client
        .put(format!("{}/v1/push-tokens", app.server_url))
        .header("Authorization", format!("Bearer {}", bob.token))
        .json(&serde_json::json!({"token": format!("token:{}", bob.user_id)}))
        .send()
        .await
        .expect("Failed to set push token");

    let alice = app.register_user(&common::generate_username("alice_push")).await;

    // 2. Consume 2 keys to trigger the PreKeyLow event (21 -> 20 -> 19)
    for _ in 0..2 {
        app.client
            .get(format!("{}/v1/keys/{}", app.server_url, bob.user_id))
            .header("Authorization", format!("Bearer {}", alice.token))
            .send()
            .await
            .expect("Failed to fetch pre-key bundle");
    }

    // 3. Wait for the push worker to process the job.
    // Since push_delay is 0, it should happen almost immediately.
    let success = app
        .wait_until(
            || async { *common::notification_counts().get(&bob.user_id).as_deref().unwrap_or(&0) > 0 },
            Duration::from_secs(5),
        )
        .await;

    assert!(success, "Bob should have received a push notification for low keys");
}

#[tokio::test]
async fn test_prekey_coalescing() {
    let app = common::TestApp::spawn_with_workers(common::get_test_config()).await;

    // 1. Bob with 25 keys
    let bob = app.register_user_with_keys(&common::generate_username("bob_burst"), 123, 25).await;
    let mut bob_ws = app.connect_ws(&bob.token).await;
    bob_ws.ensure_subscribed().await;

    let alice = app.register_user(&common::generate_username("alice_burst")).await;

    // 2. Alice fetches 10 keys CONCURRENTLY.
    // This increases the probability of multiple notifications sitting in the
    // session's channel buffer at once.
    use futures::stream::{self, StreamExt};
    stream::iter(0..10)
        .map(|_| {
            let client = &app.client;
            let url = format!("{}/v1/keys/{}", app.server_url, bob.user_id);
            let token = &alice.token;
            async move {
                client
                    .get(url)
                    .header("Authorization", format!("Bearer {token}"))
                    .send()
                    .await
                    .expect("Failed to fetch pre-key")
            }
        })
        .buffer_unordered(10)
        .collect::<Vec<_>>()
        .await;

    // Small delay to let Redis/PubSub propagate
    tokio::time::sleep(Duration::from_millis(500)).await;

    // 3. Bob should only receive a SMALL number of frames (likely 1 or 2) due to coalescing.
    // If coalescing didn't work, he'd get 10.
    let mut frames = Vec::new();
    while let Some(status) = bob_ws.receive_prekey_status_timeout(Duration::from_millis(500)).await {
        frames.push(status);
    }

    println!("Bob received {} PreKeyStatus frames", frames.len());
    assert!(frames.len() < 5, "Coalescing should have reduced the 10 notifications significantly");
    assert!(!frames.is_empty(), "Bob should have received at least one notification");

    // The last frame received should have a count below threshold
    let last = frames.last().unwrap();
    assert!(
        last.one_time_pre_key_count <= 19,
        "Count should reflect key consumption (got {})",
        last.one_time_pre_key_count
    );
}
