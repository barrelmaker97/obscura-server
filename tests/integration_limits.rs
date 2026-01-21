use obscura_server::storage::message_repo::MessageRepository;
use reqwest::StatusCode;
use uuid::Uuid;

mod common;

#[tokio::test]
async fn test_message_limit_fifo() {
    let app = common::TestApp::spawn().await;

    // Clear DB to ensure clean state (though new run_ids usually handle isolation,
    // but here we count exact messages for one user)
    sqlx::query("DELETE FROM messages").execute(&app.pool).await.unwrap();

    let run_id = Uuid::new_v4().to_string()[..8].to_string();

    let user_a = app.register_user(&format!("alice_{}", run_id)).await;
    let user_b = app.register_user(&format!("bob_{}", run_id)).await;

    // Flood 1005 messages
    for i in 0..1005 {
        let payload = format!("msg_{}", i).into_bytes();
        app.send_message(&user_a.token, user_b.user_id, &payload).await;
    }

    // Manually trigger cleanup using the repo
    let repo = MessageRepository::new(app.pool.clone());
    repo.delete_global_overflow(1000).await.expect("Failed to run cleanup");

    // Connect WS and verify first message is msg_5 (0-4 dropped)
    let mut ws = app.connect_ws(&user_b.token).await;

    if let Some(env) = ws.receive_envelope().await {
        let content = env.message.unwrap().content;
        assert_eq!(content, b"msg_5", "First message should be msg_5 (0-4 should have been pruned)");
    } else {
        panic!("No messages received");
    }
}

#[tokio::test]
async fn test_rate_limiting() {
    // 1. Setup with strict limits (1 req/s)
    let mut config = common::get_test_config();
    config.rate_limit.per_second = 1;
    config.rate_limit.burst = 1;

    let app = common::TestApp::spawn_with_config(config).await;

    // 2. First request (OK - or 401/404 but not 429)
    // We use a random token to avoid auth processing cost affecting timing too much,
    // but rate limiting happens before auth in the middleware chain.
    let resp1 = app
        .client
        .get(format!("{}/v1/gateway?token=bad", app.server_url)) // HTTP request to upgrade endpoint
        .send()
        .await
        .unwrap();

    assert_ne!(resp1.status(), StatusCode::TOO_MANY_REQUESTS);

    // 3. Second request immediately (Should be 429)
    let resp2 = app.client.get(format!("{}/v1/gateway?token=bad", app.server_url)).send().await.unwrap();

    assert_eq!(resp2.status(), StatusCode::TOO_MANY_REQUESTS);
}
