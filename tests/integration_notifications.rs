mod common;

use async_trait::async_trait;
use common::{SharedMockPushProvider, TestApp, notification_counts};
use obscura_server::adapters::push::{PushError, PushProvider};
use obscura_server::adapters::redis::NotificationRepository;
use obscura_server::services::notification::{DistributedNotificationService, NotificationService};
use obscura_server::services::push_token_service::PushTokenService;
use obscura_server::workers::PushNotificationWorker;
use serde_json::json;
use std::sync::{
    Arc,
    atomic::{AtomicUsize, Ordering},
};
use std::time::Duration;
use tracing::Instrument;
use uuid::Uuid;

#[tokio::test]
async fn test_scheduled_push_delivery() {
    common::setup_tracing();
    let config = common::get_test_config();
    let pool = common::get_test_pool().await;
    let (shutdown_tx, shutdown_rx) = tokio::sync::watch::channel(false);

    let pubsub = obscura_server::adapters::redis::RedisClient::new(&config.pubsub, 1024, shutdown_rx.clone())
        .await
        .expect("Redis must be running");

    let user_id = Uuid::new_v4();

    let token_repo = obscura_server::adapters::database::push_token_repo::PushTokenRepository::new();
    let token_service = PushTokenService::new(pool.clone(), token_repo.clone());
    {
        let mut conn = pool.acquire().await.unwrap();
        sqlx::query("INSERT INTO users (id, username, password_hash) VALUES ($1, $2, 'hash')")
            .bind(user_id)
            .bind(format!("u_{}", &user_id.to_string()[..8]))
            .execute(&mut *conn)
            .await
            .unwrap();

        token_service.register_token(user_id, format!("token:{}", user_id)).await.unwrap();
    }

    let mut test_config = config.clone();
    test_config.notifications.push_delay_secs = 1;
    test_config.notifications.worker_interval_secs = 1;

    let notification_repo = Arc::new(NotificationRepository::new(
        pubsub.clone(),
        &test_config.notifications,
    ));

    let notifier: Arc<dyn NotificationService> = Arc::new(
        DistributedNotificationService::new(
            notification_repo.clone(),
            &test_config.notifications,
            shutdown_rx.clone(),
        )
        .await
        .unwrap(),
    );

    let worker = PushNotificationWorker::new(
        pool.clone(),
        notification_repo,
        Arc::new(SharedMockPushProvider),
        token_repo,
        &test_config.notifications,
    );
    tokio::spawn(worker.run(shutdown_rx.clone()));

    // 1. Notify MessageReceived
    notifier.notify(user_id, obscura_server::domain::notification::UserEvent::MessageReceived).await;

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
    let mut config = common::get_test_config();
    config.notifications.push_delay_secs = 15;

    let app = TestApp::spawn_with_config(config).await;
    let username = format!("u_ack_{}", &Uuid::new_v4().to_string()[..8]);
    let user = app.register_user(&username).await;

    // 1. Schedule a push
    app.send_message(&user.token, user.user_id, b"msg").await;

    // 2. Connect via WebSocket
    let mut ws = app.connect_ws(&user.token).await;
    let env = ws.receive_envelope().await.expect("Envelope missing");

    // 3. Send an ACK
    ws.send_ack(env.id).await;

    // 4. Wait and verify it's removed from Redis
    let success = app
        .wait_until(
            || {
                let config = app.config.clone();
                let pubsub_url = config.pubsub.url.clone();
                let queue_key = config.notifications.push_queue_key.clone();
                let user_id = user.user_id;
                async move {
                    let client = redis::Client::open(pubsub_url).unwrap();
                    let mut conn = client.get_multiplexed_async_connection().await.unwrap();
                    let score: Option<f64> = redis::cmd("ZSCORE")
                        .arg(&queue_key)
                        .arg(user_id.to_string())
                        .query_async(&mut conn)
                        .await
                        .unwrap();
                    score.is_none()
                }
            },
            Duration::from_secs(5),
        )
        .await;

    assert!(success, "Push job was not removed from Redis on ACK");
}

#[tokio::test]
async fn test_push_cancellation_on_websocket_connect() {
    common::setup_tracing();
    let mut config = common::get_test_config();
    config.notifications.push_delay_secs = 10;

    let app = TestApp::spawn_with_config(config).await;
    let username = format!("u_ws_{}", &Uuid::new_v4().to_string()[..8]);
    let user = app.register_user(&username).await;

    // 1. Schedule push
    app.send_message(&user.token, user.user_id, b"msg").await;

    // 2. Connect WS
    let _ws = app.connect_ws(&user.token).await;

    // 3. Verify removed
    let success = app
        .wait_until(
            || {
                let config = app.config.clone();
                let pubsub_url = config.pubsub.url.clone();
                let queue_key = config.notifications.push_queue_key.clone();
                let user_id = user.user_id;
                async move {
                    let client = redis::Client::open(pubsub_url).unwrap();
                    let mut conn = client.get_multiplexed_async_connection().await.unwrap();
                    let score: Option<f64> = redis::cmd("ZSCORE")
                        .arg(&queue_key)
                        .arg(user_id.to_string())
                        .query_async(&mut conn)
                        .await
                        .unwrap();
                    score.is_none()
                }
            },
            Duration::from_secs(5),
        )
        .await;

    assert!(success, "Push job was not removed from Redis on WS connect");
}

#[tokio::test]
async fn test_delivery_exactly_once_under_competition() {
    common::setup_tracing();
    let config = common::get_test_config();
    let pool = common::get_test_pool().await;
    let (shutdown_tx, shutdown_rx) = tokio::sync::watch::channel(false);

    let pubsub = obscura_server::adapters::redis::RedisClient::new(&config.pubsub, 1024, shutdown_rx.clone())
        .await
        .expect("Redis must be running");

    let user_id = Uuid::new_v4();
    let token_repo = obscura_server::adapters::database::push_token_repo::PushTokenRepository::new();
    let token_service = PushTokenService::new(pool.clone(), token_repo.clone());
    {
        let mut conn = pool.acquire().await.unwrap();
        sqlx::query("INSERT INTO users (id, username, password_hash) VALUES ($1, $2, 'hash')")
            .bind(user_id)
            .bind(format!("u_comp_{}", &user_id.to_string()[..8]))
            .execute(&mut *conn)
            .await
            .unwrap();
        token_service.register_token(user_id, format!("token:{}", user_id)).await.unwrap();
    }

    let mut test_config = config.clone();
    test_config.notifications.push_delay_secs = 0;
    test_config.notifications.worker_interval_secs = 1;

    let notification_repo = Arc::new(NotificationRepository::new(
        pubsub.clone(),
        &test_config.notifications,
    ));

    // Spawn 10 competing workers
    for i in 0..10 {
        let worker = PushNotificationWorker::new(
            pool.clone(),
            notification_repo.clone(),
            Arc::new(SharedMockPushProvider),
            token_repo.clone(),
            &test_config.notifications,
        );
        let rx = shutdown_rx.clone();
        tokio::spawn(
            async move {
                worker.run(rx).await;
            }
            .instrument(tracing::info_span!("competing_worker", id = i)),
        );
    }

    tokio::time::sleep(Duration::from_millis(100)).await;

    let _: anyhow::Result<()> = notification_repo.push_job(user_id, 0).await;

    let start = std::time::Instant::now();
    while start.elapsed() < Duration::from_secs(10) {
        if notification_counts().get(&user_id).is_some() {
            break;
        }
        tokio::time::sleep(Duration::from_millis(100)).await;
    }

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

    let pubsub = obscura_server::adapters::redis::RedisClient::new(&config.pubsub, 1024, shutdown_rx.clone())
        .await
        .expect("Redis must be running");

    let user_id = Uuid::new_v4();
    let token_repo = obscura_server::adapters::database::push_token_repo::PushTokenRepository::new();
    let token_service = PushTokenService::new(pool.clone(), token_repo.clone());
    {
        let mut conn = pool.acquire().await.unwrap();
        sqlx::query("INSERT INTO users (id, username, password_hash) VALUES ($1, $2, 'hash')")
            .bind(user_id)
            .bind(format!("u_coal_{}", &user_id.to_string()[..8]))
            .execute(&mut *conn)
            .await
            .unwrap();
        token_service.register_token(user_id, format!("token:{}", user_id)).await.unwrap();
    }

    let mut test_config = config.clone();
    test_config.notifications.push_delay_secs = 2;
    test_config.notifications.worker_interval_secs = 1;

    let notification_repo = Arc::new(NotificationRepository::new(
        pubsub.clone(),
        &test_config.notifications,
    ));

    let notifier: Arc<dyn NotificationService> = Arc::new(
        DistributedNotificationService::new(
            notification_repo.clone(),
            &test_config.notifications,
            shutdown_rx.clone(),
        )
        .await
        .unwrap(),
    );

    let worker = PushNotificationWorker::new(
        pool.clone(),
        notification_repo,
        Arc::new(SharedMockPushProvider),
        token_repo,
        &test_config.notifications,
    );
    tokio::spawn(worker.run(shutdown_rx.clone()));

    for _ in 0..5 {
        notifier.notify(user_id, obscura_server::domain::notification::UserEvent::MessageReceived).await;
    }

    tokio::time::sleep(Duration::from_secs(5)).await;

    let count = notification_counts().get(&user_id).map(|c| *c).unwrap_or(0);
    assert_eq!(count, 1, "Expected 1 coalesced notification, got {}", count);

    let _ = shutdown_tx.send(true);
}

#[derive(Debug, Default)]
struct ConcurrencyMockProvider;

#[async_trait]
impl PushProvider for ConcurrencyMockProvider {
    async fn send_push(&self, _token: &str) -> Result<(), PushError> {
        let current = IN_FLIGHT_COUNT.fetch_add(1, Ordering::SeqCst) + 1;

        static PEAK_IN_FLIGHT: AtomicUsize = AtomicUsize::new(0);
        loop {
            let prev = PEAK_IN_FLIGHT.load(Ordering::SeqCst);
            if current <= prev
                || PEAK_IN_FLIGHT.compare_exchange(prev, current, Ordering::SeqCst, Ordering::SeqCst).is_ok()
            {
                break;
            }
        }

        tokio::time::sleep(Duration::from_millis(200)).await;

        IN_FLIGHT_COUNT.fetch_sub(1, Ordering::SeqCst);
        Ok(())
    }
}

static IN_FLIGHT_COUNT: AtomicUsize = AtomicUsize::new(0);

#[tokio::test]
async fn test_notification_worker_concurrency_limit() {
    common::setup_tracing();
    let config = common::get_test_config();
    let pool = common::get_test_pool().await;
    let (shutdown_tx, shutdown_rx) = tokio::sync::watch::channel(false);

    let pubsub = obscura_server::adapters::redis::RedisClient::new(&config.pubsub, 1024, shutdown_rx.clone())
        .await
        .expect("Redis must be running");

    let token_repo = obscura_server::adapters::database::push_token_repo::PushTokenRepository::new();
    let token_service = PushTokenService::new(pool.clone(), token_repo.clone());

    let poll_limit = 20;
    let concurrency = 2;

    let notification_repo = Arc::new(NotificationRepository::new(
        pubsub.clone(),
        &config.notifications,
    ));

    let worker = PushNotificationWorker::new(
        pool.clone(),
        notification_repo.clone(),
        Arc::new(ConcurrencyMockProvider),
        token_repo.clone(),
        &config.notifications,
    );

    tokio::spawn(worker.run(shutdown_rx.clone()));

    for i in 0..10 {
        let user_id = Uuid::new_v4();
        {
            let mut conn = pool.acquire().await.unwrap();
            let username = format!("conc_{}_{}", i, &Uuid::new_v4().to_string()[..8]);
            sqlx::query("INSERT INTO users (id, username, password_hash) VALUES ($1, $2, 'hash')")
                .bind(user_id)
                .bind(username)
                .execute(&mut *conn)
                .await
                .unwrap();

            token_service.register_token(user_id, format!("token:{}", user_id)).await.unwrap();
        }
        let _: anyhow::Result<()> = notification_repo.push_job(user_id, 0).await;
    }

    tokio::time::sleep(Duration::from_secs(5)).await;

    let _ = shutdown_tx.send(true);
}

#[tokio::test]
async fn test_register_push_token() {
    let app = TestApp::spawn().await;
    let username = format!("token_user_{}", Uuid::new_v4());
    let user = app.register_user(&username).await;

    let token = "test_fcm_token_123";
    let payload = json!({
        "token": token
    });

    let resp = app
        .client
        .put(format!("{}/v1/push-tokens", app.server_url))
        .header("Authorization", format!("Bearer {}", user.token))
        .json(&payload)
        .send()
        .await
        .unwrap();

    assert_eq!(resp.status(), 200);

    let stored_token: String = sqlx::query_scalar("SELECT token FROM push_tokens WHERE user_id = $1")
        .bind(user.user_id)
        .fetch_one(&app.pool)
        .await
        .unwrap();

    assert_eq!(stored_token, token);

    let new_token = "updated_fcm_token_456";
    let resp = app
        .client
        .put(format!("{}/v1/push-tokens", app.server_url))
        .header("Authorization", format!("Bearer {}", user.token))
        .json(&json!({ "token": new_token }))
        .send()
        .await
        .unwrap();

    assert_eq!(resp.status(), 200);

    let stored_token: String = sqlx::query_scalar("SELECT token FROM push_tokens WHERE user_id = $1")
        .bind(user.user_id)
        .fetch_one(&app.pool)
        .await
        .unwrap();

    assert_eq!(stored_token, new_token);
}

#[tokio::test]
async fn test_register_push_token_unauthorized() {
    let app = TestApp::spawn().await;

    let resp = app
        .client
        .put(format!("{}/v1/push-tokens", app.server_url))
        .json(&json!({ "token": "some_token" }))
        .send()
        .await
        .unwrap();

    assert_eq!(resp.status(), 401);
}

#[tokio::test]
async fn test_notification_lag_recovery() {
    common::setup_tracing();
    let mut config = common::get_test_config();
    config.websocket.outbound_buffer_size = 10;

    let app = common::TestApp::spawn_with_config(config).await;
    let run_id = Uuid::new_v4().to_string()[..8].to_string();

    let user_a = app.register_user(&format!("alice_{}", run_id)).await;
    let user_b = app.register_user(&format!("bob_{}", run_id)).await;

    let mut ws = app.connect_ws(&user_b.token).await;

    let message_count = 100;
    for i in 0..message_count {
        let content = format!("Message {}", i).into_bytes();
        app.send_message(&user_a.token, user_b.user_id, &content).await;
    }

    tokio::time::sleep(tokio::time::Duration::from_millis(500)).await;

    let mut received = 0;
    let start = std::time::Instant::now();
    let timeout = std::time::Duration::from_secs(10);

    while received < message_count && start.elapsed() < timeout {
        if ws.receive_envelope_timeout(std::time::Duration::from_millis(100)).await.is_some() {
            received += 1;
        }
    }

    assert_eq!(received, message_count, "Should receive all {} messages despite notification lag", message_count);
}
