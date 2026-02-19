mod common;

use async_trait::async_trait;
use obscura_server::adapters::database::push_token_repo::PushTokenRepository;
use obscura_server::adapters::push::{PushError, PushProvider};
use obscura_server::adapters::redis::NotificationRepository;
use obscura_server::workers::PushNotificationWorker;
use std::sync::Arc;
use uuid::Uuid;

#[derive(Debug, Default)]
struct FailingPushProvider;

#[async_trait]
impl PushProvider for FailingPushProvider {
    async fn send_push(&self, _token: &str) -> Result<(), PushError> {
        Err(PushError::Unregistered)
    }
}

#[tokio::test]
async fn test_push_worker_invalidates_unregistered_tokens() {
    common::setup_tracing();
    let mut config = common::get_test_config();
    // Speed up intervals for the test
    config.notifications.worker_interval_secs = 1;
    config.notifications.janitor_interval_secs = 1;
    // Use a unique queue key for this test to avoid competition with the default TestApp worker
    config.notifications.push_queue_key = format!("{}-janitor", config.notifications.push_queue_key);

    let pool = common::get_test_pool().await;
    let user_id = Uuid::new_v4();
    let token = "invalid_token_123";

    // 1. Setup DB state
    {
        sqlx::query("INSERT INTO users (id, username, password_hash) VALUES ($1, $2, 'hash')")
            .bind(user_id)
            .bind(format!("fail_{}", &user_id.to_string()[..8]))
            .execute(&pool)
            .await
            .unwrap();

        let repo = PushTokenRepository::new();
        let mut conn = pool.acquire().await.unwrap();
        repo.upsert_token(&mut conn, user_id, token).await.unwrap();
    }

    // 2. Schedule a push
    let (shutdown_tx, shutdown_rx) = tokio::sync::watch::channel(false);
    let pubsub =
        obscura_server::adapters::redis::RedisClient::new(&config.pubsub, 1024, shutdown_rx.clone()).await.unwrap();
    let notification_repo = Arc::new(NotificationRepository::new(pubsub.clone(), &config.notifications));
    let _: anyhow::Result<()> = notification_repo.push_job(user_id, 0).await;

    // 3. Setup Worker with FAILING provider and START it
    let worker = PushNotificationWorker::new(
        pool.clone(),
        notification_repo,
        Arc::new(FailingPushProvider),
        PushTokenRepository::new(),
        &config.notifications,
    );

    let worker_handle = tokio::spawn(worker.run(shutdown_rx));

    // 4. Verify token is AUTOMATICALLY DELETED from database by the worker loop
    let mut success = false;
    let start = std::time::Instant::now();
    while start.elapsed() < std::time::Duration::from_secs(10) {
        let exists: bool = sqlx::query_scalar("SELECT EXISTS(SELECT 1 FROM push_tokens WHERE token = $1)")
            .bind(token)
            .fetch_one(&pool)
            .await
            .unwrap();
        if !exists {
            success = true;
            break;
        }
        tokio::time::sleep(std::time::Duration::from_millis(100)).await;
    }

    // Cleanup
    let _ = shutdown_tx.send(true);
    let _ = worker_handle.await;

    assert!(success, "Token should have been deleted automatically by the integrated worker loop");
}

#[derive(Debug, Default)]
struct MockPushProvider;

#[async_trait]
impl PushProvider for MockPushProvider {
    async fn send_push(&self, _token: &str) -> Result<(), PushError> {
        Ok(())
    }
}

#[tokio::test]
async fn test_push_worker_removes_job_when_user_has_no_token() {
    common::setup_tracing();
    let mut config = common::get_test_config();
    // Speed up intervals for the test
    config.notifications.worker_interval_secs = 1;
    // Set visibility timeout to small value so job reappears quickly if not deleted
    config.notifications.visibility_timeout_secs = 2;
    // Use a unique queue key for this test
    config.notifications.push_queue_key = format!("{}-no-token", config.notifications.push_queue_key);

    let pool = common::get_test_pool().await;
    let user_id = Uuid::new_v4();

    // 1. Setup DB state (User ONLY, NO token)
    {
        sqlx::query("INSERT INTO users (id, username, password_hash) VALUES ($1, $2, 'hash')")
            .bind(user_id)
            .bind(format!("user_{}", &user_id.to_string()[..8]))
            .execute(&pool)
            .await
            .unwrap();
    }

    // 2. Schedule a push job
    let (shutdown_tx, shutdown_rx) = tokio::sync::watch::channel(false);
    let redis_client =
        obscura_server::adapters::redis::RedisClient::new(&config.pubsub, 1024, shutdown_rx.clone()).await.unwrap();
    let notification_repo = Arc::new(NotificationRepository::new(redis_client.clone(), &config.notifications));

    // Push the job
    notification_repo.push_job(user_id, 0).await.unwrap();

    // 3. Start Worker
    let worker = PushNotificationWorker::new(
        pool.clone(),
        notification_repo.clone(),
        Arc::new(MockPushProvider),
        PushTokenRepository::new(),
        &config.notifications,
    );

    let worker_handle = tokio::spawn(worker.run(shutdown_rx));

    // 4. Wait for worker to process.
    // T=0: Job visible. Worker picks it up. Leases for 2s.
    // T=2: Job becomes visible again if not deleted.
    // T=3: Worker would pick it up again if we let it, but we just want to check existence.
    // Let's wait 4s to be sure it has reappeared (if not deleted).
    tokio::time::sleep(std::time::Duration::from_secs(4)).await;

    // 5. Check if job is gone from Redis
    // If we can lease it, it's still there.
    let leased = notification_repo.lease_due_jobs(10, 0).await.unwrap();

    // Cleanup
    let _ = shutdown_tx.send(true);
    let _ = worker_handle.await;

    // If existing code is buggy, leased will NOT be empty (it will contain the job).
    // If fixed, leased will be empty.
    assert!(leased.is_empty(), "Job should have been removed because user has no token, but it was still present/reappeared: {:?}", leased);
}
