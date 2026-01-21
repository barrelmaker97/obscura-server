use uuid::Uuid;

mod common;

#[tokio::test]
async fn test_prekey_status_low_keys() {
    let app = common::TestApp::spawn().await;
    let run_id = Uuid::new_v4().to_string()[..8].to_string();
    let username = format!("status_user_low_{}", run_id);

    // 1. Register with 0 one-time keys (below threshold of 20)
    let user = app.register_user_with_keys(&username, 123, 0).await;

    // 2. Connect WebSocket
    let mut ws = app.connect_ws(&user.token).await;

    // 3. Expect PreKeyStatus message immediately
    let status = ws.receive_prekey_status().await.expect("Did not receive PreKeyStatus");
    assert_eq!(status.one_time_pre_key_count, 0);
    assert_eq!(status.min_threshold, 20);
}

#[tokio::test]
async fn test_prekey_status_sufficient_keys() {
    let app = common::TestApp::spawn().await;
    let run_id = Uuid::new_v4().to_string()[..8].to_string();
    let username = format!("status_user_ok_{}", run_id);

    // 1. Register with 25 one-time keys (above threshold of 20)
    let user = app.register_user_with_keys(&username, 123, 25).await;

    // 2. Connect WebSocket
    let mut ws = app.connect_ws(&user.token).await;

    // 3. Expect NO PreKeyStatus message
    let status = ws.receive_prekey_status_timeout(std::time::Duration::from_millis(500)).await;
    assert!(status.is_none(), "Received PreKeyStatus unexpectedly!");
}