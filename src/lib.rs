#![forbid(unsafe_code)]
#![warn(clippy::all)]
#![warn(clippy::pedantic)]
#![warn(clippy::nursery)]
#![warn(clippy::unwrap_used)]
#![warn(clippy::todo)]
#![warn(clippy::panic)]
#![warn(clippy::dbg_macro)]
#![warn(clippy::print_stdout)]
#![warn(clippy::print_stderr)]
#![warn(clippy::clone_on_ref_ptr)]
#![warn(unreachable_pub)]
#![warn(missing_debug_implementations)]
#![warn(unused_qualifications)]
#![deny(unused_must_use)]

pub mod adapters;
pub mod api;
pub mod config;
pub mod domain;
pub mod error;
pub mod proto;
pub mod services;
pub mod telemetry;
pub mod workers;

use crate::adapters::database::attachment_repo::AttachmentRepository;
use crate::adapters::database::key_repo::KeyRepository;
use crate::adapters::database::message_repo::MessageRepository;
use crate::adapters::database::push_token_repo::PushTokenRepository;
use crate::adapters::database::refresh_token_repo::RefreshTokenRepository;
use crate::adapters::database::user_repo::UserRepository;
use crate::adapters::push::PushProvider;
use crate::api::ServiceContainer;
use crate::config::{Config, StorageConfig};
use crate::services::account_service::AccountService;
use crate::services::attachment_service::AttachmentService;
use crate::services::auth_service::AuthService;
use crate::services::crypto_service::CryptoService;
use crate::services::gateway::GatewayService;
use crate::services::health_service::HealthService;
use crate::services::key_service::KeyService;
use crate::services::message_service::MessageService;
use crate::services::notification::{DistributedNotificationService, NotificationService};
use crate::services::push_token_service::PushTokenService;
use crate::services::rate_limit_service::RateLimitService;
use crate::workers::{AttachmentCleanupWorker, MessageCleanupWorker, PushNotificationWorker};
use std::sync::Arc;
use tokio::sync::watch;

/// Initializes all repositories and services, and spawns background workers.
///
/// # Errors
/// Returns an error if any service fails to initialize (e.g. `PubSub` connection).
#[tracing::instrument(skip(pool, pubsub, s3_client, push_provider, config, shutdown_rx))]
pub async fn init_application(
    pool: adapters::database::DbPool,
    pubsub: Arc<adapters::redis::RedisClient>,
    s3_client: aws_sdk_s3::Client,
    push_provider: Arc<dyn PushProvider>,
    config: &Config,
    shutdown_rx: watch::Receiver<bool>,
) -> anyhow::Result<(ServiceContainer, HealthService, Vec<tokio::task::JoinHandle<()>>)> {
    // Initialize Repositories
    let key_repo = KeyRepository::new();
    let message_repo = MessageRepository::new();
    let user_repo = UserRepository::new();
    let refresh_repo = RefreshTokenRepository::new();
    let attachment_repo = AttachmentRepository::new();
    let push_token_repo = PushTokenRepository::new();

    // Initialize Specialized Services
    let push_token_service = PushTokenService::new(pool.clone(), push_token_repo.clone());
    let notification_repo =
        Arc::new(adapters::redis::NotificationRepository::new(Arc::clone(&pubsub), &config.notifications));
    let notifier: Arc<dyn NotificationService> = Arc::new(
        DistributedNotificationService::new(Arc::clone(&notification_repo), &config.notifications, shutdown_rx.clone())
            .await?,
    );

    // Initialize Core Services
    let crypto_service = CryptoService::new();
    let key_service = KeyService::new(pool.clone(), key_repo, crypto_service, config.messaging.clone());
    let auth_service = AuthService::new(config.auth.clone(), pool.clone(), user_repo.clone(), refresh_repo);
    let message_service = MessageService::new(
        pool.clone(),
        message_repo.clone(),
        Arc::clone(&notifier),
        config.messaging.clone(),
        config.ttl_days,
    );
    let account_service = AccountService::new(
        pool.clone(),
        user_repo,
        message_repo.clone(),
        auth_service.clone(),
        key_service.clone(),
        Arc::clone(&notifier),
    );
    let gateway_service = GatewayService::new(
        message_service.clone(),
        key_service.clone(),
        Arc::clone(&notifier),
        config.websocket.clone(),
    );
    let attachment_service = AttachmentService::new(
        pool.clone(),
        attachment_repo.clone(),
        s3_client.clone(),
        config.storage.clone(),
        config.ttl_days,
    );
    let rate_limit_service = RateLimitService::new(config.server.trusted_proxies.clone());
    let health_service = HealthService::new(
        pool.clone(),
        s3_client.clone(),
        Arc::clone(&pubsub),
        config.storage.bucket.clone(),
        config.health.clone(),
    );

    let services = ServiceContainer {
        pool,
        key_service,
        attachment_service,
        account_service,
        auth_service,
        message_service,
        gateway_service,
        notification_service: notifier,
        push_token_service,
        rate_limit_service,
    };

    // Spawn Workers
    let mut tasks = Vec::new();

    let message_worker = MessageCleanupWorker::new(services.pool.clone(), message_repo, config.messaging.clone());
    let message_rx = shutdown_rx.clone();
    tasks.push(tokio::spawn(async move { message_worker.run(message_rx).await }));

    let attachment_worker =
        AttachmentCleanupWorker::new(services.pool.clone(), attachment_repo, s3_client, config.storage.clone());
    let attachment_rx = shutdown_rx.clone();
    tasks.push(tokio::spawn(async move { attachment_worker.run(attachment_rx).await }));

    let push_worker = PushNotificationWorker::new(
        services.pool.clone(),
        notification_repo,
        push_provider,
        push_token_repo,
        &config.notifications,
    );
    tasks.push(tokio::spawn(async move { push_worker.run(shutdown_rx).await }));

    Ok((services, health_service, tasks))
}

/// Runs database migrations.
///
/// # Errors
/// Returns an error if migrations fail.
#[tracing::instrument(skip(pool))]
pub async fn run_migrations(pool: &adapters::database::DbPool) -> anyhow::Result<()> {
    sqlx::migrate!().run(pool).await.map_err(Into::into)
}

/// Initializes an S3 client from configuration.
#[tracing::instrument(skip(config))]
pub async fn init_s3_client(config: &StorageConfig) -> aws_sdk_s3::Client {
    let region_provider = aws_config::Region::new(config.region.clone());
    let mut config_loader = aws_config::defaults(aws_config::BehaviorVersion::latest()).region(region_provider);

    if let Some(ref endpoint) = config.endpoint {
        config_loader = config_loader.endpoint_url(endpoint);
    }

    if let (Some(ak), Some(sk)) = (&config.access_key, &config.secret_key) {
        let creds = aws_credential_types::Credentials::new(ak.clone(), sk.clone(), None, None, "static");
        config_loader = config_loader.credentials_provider(creds);
    }

    let sdk_config = config_loader.load().await;
    let s3_config_builder = aws_sdk_s3::config::Builder::from(&sdk_config).force_path_style(config.force_path_style);
    aws_sdk_s3::Client::from_conf(s3_config_builder.build())
}

/// Sets up a panic hook that logs the panic message and location.
pub fn setup_panic_hook() {
    std::panic::set_hook(Box::new(|panic_info| {
        let payload = panic_info.payload();
        let msg = payload
            .downcast_ref::<&str>()
            .map_or_else(|| payload.downcast_ref::<String>().map_or_else(|| "Box<Any>", String::as_str), |s| *s);

        let location = panic_info.location().map_or_else(
            || "unknown".to_string(),
            |location| format!("{}:{}:{}", location.file(), location.line(), location.column()),
        );

        tracing::error!(
            panic.message = %msg,
            panic.location = %location,
            "Application panicked"
        );
    }));
}

/// Returns a future that completes when a termination signal is received.
///
/// # Panics
/// Panics if the signal handlers cannot be installed.
pub async fn shutdown_signal() {
    let ctrl_c = async {
        tokio::signal::ctrl_c().await.expect("failed to install Ctrl+C handler");
    };

    #[cfg(unix)]
    let terminate = async {
        tokio::signal::unix::signal(tokio::signal::unix::SignalKind::terminate())
            .expect("failed to install signal handler")
            .recv()
            .await;
    };

    #[cfg(not(unix))]
    let terminate = std::future::pending::<()>();

    tokio::select! {
        () = ctrl_c => {},
        () = terminate => {},
    }

    tracing::info!("Shutdown signal received, starting graceful shutdown...");
}

/// Spawns a task that listens for OS signals and broadcasts a shutdown signal.
pub fn spawn_signal_handler(shutdown_tx: watch::Sender<bool>) {
    tokio::spawn(async move {
        shutdown_signal().await;
        let _ = shutdown_tx.send(true);
    });
}
