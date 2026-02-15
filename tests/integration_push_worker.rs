mod common;

use async_trait::async_trait;
use obscura_server::adapters::database::push_token_repo::PushTokenRepository;
use obscura_server::services::notification::provider::{PushError, PushProvider};
use obscura_server::services::notification::scheduler::NotificationScheduler;
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
            .bind(format!("push_fail_user_{}", &user_id.to_string()[..8]))
            .execute(&pool)
            .await
            .unwrap();

        let repo = PushTokenRepository::new();
        let mut conn = pool.acquire().await.unwrap();
        repo.upsert_token(&mut conn, user_id, token).await.unwrap();
    }

    // 2. Schedule a push
    let scheduler = Arc::new(NotificationScheduler::new(
        obscura_server::adapters::redis::RedisClient::new(&config.pubsub, 1024, tokio::sync::watch::channel(false).1).await.unwrap(),
        config.notifications.push_queue_key.clone(),
    ));
    scheduler.schedule_push(user_id, 0).await.expect("Failed to schedule push");

    // 3. Setup Worker with FAILING provider
    let worker = PushNotificationWorker::new(
        pool.clone(),
        scheduler,
        Arc::new(FailingPushProvider),
        PushTokenRepository::new(),
        10,
        1,
        1,
    );

    // 4. Run one iteration of the worker
    worker.process_due_jobs().await.expect("Worker iteration failed");

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
