mod common;

use async_trait::async_trait;
use common::TestApp;
use dashmap::DashMap;
use obscura_server::services::notification::provider::{PushProvider, PushError};
use obscura_server::services::notification::{DistributedNotificationService, NotificationService};
use std::sync::{Arc, OnceLock};
use std::time::Duration;
use uuid::Uuid;
use tracing::Instrument;

/// Shared global state for all test providers to report to.
/// This allows parallel tests to work even if Worker A picks up User B's job.
fn notification_counts() -> &'static DashMap<Uuid, u32> {
    static COUNTS: OnceLock<DashMap<Uuid, u32>> = OnceLock::new();
    COUNTS.get_or_init(DashMap::new)
}

#[derive(Debug, Default)]
struct SharedMockPushProvider;

#[async_trait]
impl PushProvider for SharedMockPushProvider {
    async fn send_push(&self, token: &str) -> Result<(), PushError> {
        if let Some(user_id_str) = token.strip_prefix("token:") {
            if let Ok(user_id) = Uuid::parse_str(user_id_str) {
                *notification_counts().entry(user_id).or_insert(0) += 1;
            }
        }
        Ok(())
    }
}

#[tokio::test]
async fn test_scheduled_push_delivery() {
    common::setup_tracing();
    let config = common::get_test_config();
    let pool = common::get_test_pool().await;
    let (shutdown_tx, shutdown_rx) = tokio::sync::watch::channel(false);
    
    let pubsub = obscura_server::adapters::redis::RedisClient::new(
        &config.pubsub,
        1024,
        shutdown_rx.clone()
    ).await.expect("Redis must be running");
    
    let user_id = Uuid::new_v4();

    // Register a token manually in DB for this user
    let token_repo = obscura_server::adapters::database::push_token_repo::PushTokenRepository::new();
    {
        let mut conn = pool.acquire().await.unwrap();
        // We need a real user first due to FK
        sqlx::query("INSERT INTO users (id, username, password_hash) VALUES ($1, $2, 'hash')")
            .bind(user_id)
            .bind(format!("user_{}", user_id))
            .execute(&mut *conn).await.unwrap();
            
        token_repo.upsert_token(&mut conn, user_id, &format!("token:{}", user_id)).await.unwrap();
    }

    let mut test_config = config.clone();
    test_config.notifications.push_delay_secs = 1; 

    let notifier: Arc<dyn NotificationService> = Arc::new(
        DistributedNotificationService::new(
            pubsub.clone(),
            &test_config,
            shutdown_rx.clone(),
            Some(Arc::new(SharedMockPushProvider) as Arc<dyn PushProvider>),
            token_repo,
            pool.clone()
        ).await.unwrap()
    );

    // 1. Notify MessageReceived
    notifier.notify(user_id, obscura_server::services::notification::UserEvent::MessageReceived).await;

    // 2. Wait for push
    let start = std::time::Instant::now();
    let mut delivered = false;
    while start.elapsed() < Duration::from_secs(10) {
        if notification_counts().get(&user_id).map(|c| *c).unwrap_or(0) > 0 {
            delivered = true;
            break;
        }
        tokio::time::sleep(Duration::from_millis(100)).await;
    }

    assert!(delivered, "Push notification was not delivered for unique user {}", user_id);
    assert_eq!(*notification_counts().get(&user_id).unwrap(), 1);
    
    let _ = shutdown_tx.send(true);
}

#[tokio::test]
async fn test_push_cancellation_on_ack() {
    common::setup_tracing();
    let config = common::get_test_config();
    let pool = common::get_test_pool().await;
    let (shutdown_tx, shutdown_rx) = tokio::sync::watch::channel(false);
    
    let pubsub = obscura_server::adapters::redis::RedisClient::new(
        &config.pubsub,
        1024,
        shutdown_rx.clone()
    ).await.expect("Redis must be running");
    
    let user_id = Uuid::new_v4();
    let token_repo = obscura_server::adapters::database::push_token_repo::PushTokenRepository::new();
    {
        let mut conn = pool.acquire().await.unwrap();
        sqlx::query("INSERT INTO users (id, username, password_hash) VALUES ($1, $2, 'hash')")
            .bind(user_id)
            .bind(format!("user_{}", user_id))
            .execute(&mut *conn).await.unwrap();
        token_repo.upsert_token(&mut conn, user_id, &format!("token:{}", user_id)).await.unwrap();
    }

    let mut test_config = config.clone();
    test_config.notifications.push_delay_secs = 5;

    let notifier: Arc<dyn NotificationService> = Arc::new(
        DistributedNotificationService::new(
            pubsub.clone(),
            &test_config,
            shutdown_rx.clone(),
            Some(Arc::new(SharedMockPushProvider) as Arc<dyn PushProvider>),
            token_repo,
            pool.clone()
        ).await.unwrap()
    );

    // 1. Notify MessageReceived
    notifier.notify(user_id, obscura_server::services::notification::UserEvent::MessageReceived).await;

    // 2. Immediately cancel
    notifier.cancel_pending_notifications(user_id).await;

    // 3. Wait and ensure NO push is delivered
    tokio::time::sleep(Duration::from_secs(3)).await;
    assert_eq!(notification_counts().get(&user_id).map(|c| *c).unwrap_or(0), 0, "Push was sent despite cancellation");
    
    let _ = shutdown_tx.send(true);
}

#[tokio::test]
async fn test_push_cancellation_on_websocket_connect() {
    common::setup_tracing();
    let mut config = common::get_test_config();
    config.notifications.push_delay_secs = 10;

    let app = TestApp::spawn_with_config(config).await;
    
    let (shutdown_tx, shutdown_rx) = tokio::sync::watch::channel(false);
    let pubsub = obscura_server::adapters::redis::RedisClient::new(
        &app.config.pubsub,
        1024,
        shutdown_rx.clone()
    ).await.unwrap();

    let username = format!("u_{}", &Uuid::new_v4().to_string()[..8]);
    let user = app.register_user(&username).await;

    let mut conn = pubsub.publisher();
    let now = time::OffsetDateTime::now_utc().unix_timestamp();
    let run_at = now + 20; // 20 seconds in future
    
    redis::cmd("ZADD")
        .arg("jobs:push_notifications")
        .arg(run_at as f64)
        .arg(user.user_id.to_string())
        .query_async::<i64>(&mut conn)
        .await.unwrap();

    let _ws = app.connect_ws(&user.token).await;

    let success = app.wait_until(|| {
        let pubsub = pubsub.clone();
        let user_id = user.user_id;
        async move {
            let mut conn = pubsub.publisher();
            let score: Option<f64> = redis::cmd("ZSCORE")
                .arg("jobs:push_notifications")
                .arg(user_id.to_string())
                .query_async(&mut conn)
                .await.unwrap();
            score.is_none()
        }
    }, Duration::from_secs(5)).await;

    assert!(success, "Push job for user {} was not removed from Redis on WS connect", user.user_id);
    
    let _ = shutdown_tx.send(true);
}

#[tokio::test]
async fn test_delivery_exactly_once_under_competition() {
    common::setup_tracing();
    let config = common::get_test_config();
    let pool = common::get_test_pool().await;
    let (shutdown_tx, shutdown_rx) = tokio::sync::watch::channel(false);
    
    let pubsub = obscura_server::adapters::redis::RedisClient::new(
        &config.pubsub,
        1024,
        shutdown_rx.clone()
    ).await.expect("Redis must be running");
    
    let user_id = Uuid::new_v4();
    let token_repo = obscura_server::adapters::database::push_token_repo::PushTokenRepository::new();
    {
        let mut conn = pool.acquire().await.unwrap();
        sqlx::query("INSERT INTO users (id, username, password_hash) VALUES ($1, $2, 'hash')")
            .bind(user_id)
            .bind(format!("user_{}", user_id))
            .execute(&mut *conn).await.unwrap();
        token_repo.upsert_token(&mut conn, user_id, &format!("token:{}", user_id)).await.unwrap();
    }

    let mut test_config = config.clone();
    test_config.notifications.push_delay_secs = 0; // Immediate for this test

    let scheduler = std::sync::Arc::new(
        obscura_server::services::notification::scheduler::NotificationScheduler::new(pubsub.clone())
    );

    // Spawn 10 competing workers
    for i in 0..10 {
        let worker = obscura_server::services::notification::worker::NotificationWorker::new(
            pool.clone(),
            scheduler.clone(),
            Arc::new(SharedMockPushProvider),
            token_repo.clone()
        );
        let rx = shutdown_rx.clone();
        tokio::spawn(async move {
            worker.run(rx).await;
        }.instrument(tracing::info_span!("competing_worker", id = i)));
    }

    // Give workers a moment to start
    tokio::time::sleep(Duration::from_millis(100)).await;

    // Schedule 1 push
    scheduler.schedule_push(user_id, 0).await.unwrap();

    // Wait for delivery
    let start = std::time::Instant::now();
    while start.elapsed() < Duration::from_secs(5) {
        if notification_counts().get(&user_id).is_some() {
            break;
        }
        tokio::time::sleep(Duration::from_millis(50)).await;
    }

    // ASSERT: Exactly one worker should have succeeded
    let count = notification_counts().get(&user_id).map(|c| *c).unwrap_or(0);
    assert_eq!(count, 1, "Notification delivered {} times, expected exactly 1", count);
    
    let _ = shutdown_tx.send(true);
}

#[tokio::test]
async fn test_push_coalescing() {
    common::setup_tracing();
    let config = common::get_test_config();
    let pool = common::get_test_pool().await;
    let (shutdown_tx, shutdown_rx) = tokio::sync::watch::channel(false);
    
    let pubsub = obscura_server::adapters::redis::RedisClient::new(
        &config.pubsub,
        1024,
        shutdown_rx.clone()
    ).await.expect("Redis must be running");
    
    let user_id = Uuid::new_v4();
    let token_repo = obscura_server::adapters::database::push_token_repo::PushTokenRepository::new();
    {
        let mut conn = pool.acquire().await.unwrap();
        sqlx::query("INSERT INTO users (id, username, password_hash) VALUES ($1, $2, 'hash')")
            .bind(user_id)
            .bind(format!("user_{}", user_id))
            .execute(&mut *conn).await.unwrap();
        token_repo.upsert_token(&mut conn, user_id, &format!("token:{}", user_id)).await.unwrap();
    }

    let mut test_config = config.clone();
    test_config.notifications.push_delay_secs = 2; // Long enough to send multiple

    let notifier: Arc<dyn NotificationService> = Arc::new(
        DistributedNotificationService::new(
            pubsub.clone(),
            &test_config,
            shutdown_rx.clone(),
            Some(Arc::new(SharedMockPushProvider) as Arc<dyn PushProvider>),
            token_repo,
            pool.clone()
        ).await.unwrap()
    );

    // 1. Notify multiple times rapidly
    for _ in 0..5 {
        notifier.notify(user_id, obscura_server::services::notification::UserEvent::MessageReceived).await;
    }

    // 2. Wait for push
    tokio::time::sleep(Duration::from_secs(5)).await;

    // 3. ASSERT: Coalesced into exactly one delivery
    let count = notification_counts().get(&user_id).map(|c| *c).unwrap_or(0);
    assert_eq!(count, 1, "Expected 1 coalesced notification, got {}", count);
    
    let _ = shutdown_tx.send(true);
}
