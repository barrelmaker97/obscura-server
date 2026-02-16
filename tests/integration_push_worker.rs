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
    let config = common::get_test_config();
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
    let pubsub =
        obscura_server::adapters::redis::RedisClient::new(&config.pubsub, 1024, tokio::sync::watch::channel(false).1)
            .await
            .unwrap();
    let notification_repo = Arc::new(NotificationRepository::new(pubsub.clone(), &config.notifications));
    let _: anyhow::Result<()> = notification_repo.push_job(user_id, 0).await;

    // 3. Setup Worker with FAILING provider
    let worker = PushNotificationWorker::new(
        pool.clone(),
        notification_repo,
        Arc::new(FailingPushProvider),
        PushTokenRepository::new(),
        &config.notifications,
    );

    // 4. Run one iteration of the worker
    let (tx, mut rx) = tokio::sync::mpsc::channel(10);
    worker.process_due_jobs(tx).await.expect("Worker iteration failed");

    // Manually trigger the janitor behavior by forwarding the token
    // (In a real run, the worker loop would handle this)
    if let Some(t) = rx.recv().await {
        let mut conn = pool.acquire().await.unwrap();
        obscura_server::adapters::database::push_token_repo::PushTokenRepository::new()
            .delete_tokens_batch(&mut conn, &[t])
            .await
            .unwrap();
    }

    // 5. Verify token is DELETED from database
    // Note: Since spawn is used inside process_due_jobs, we might need a small wait
    let mut success = false;
    for _ in 0..50 {
        let exists: bool = sqlx::query_scalar("SELECT EXISTS(SELECT 1 FROM push_tokens WHERE token = $1)")
            .bind(token)
            .fetch_one(&pool)
            .await
            .unwrap();
        if !exists {
            success = true;
            break;
        }
        tokio::time::sleep(std::time::Duration::from_millis(50)).await;
    }

    assert!(success, "Token should have been deleted after provider returned Unregistered error");
}
