use obscura_server::adapters::database::attachment_repo::AttachmentRepository;
use obscura_server::adapters::database::key_repo::KeyRepository;
use obscura_server::adapters::database::message_repo::MessageRepository;
use obscura_server::adapters::database::push_token_repo::PushTokenRepository;
use obscura_server::adapters::database::refresh_token_repo::RefreshTokenRepository;
use obscura_server::adapters::database::user_repo::UserRepository;
use obscura_server::api::{self, MgmtState, ServiceContainer};
use obscura_server::config::{Config, StorageConfig};
use obscura_server::services::account_service::AccountService;
use obscura_server::services::attachment_service::AttachmentService;
use obscura_server::services::auth_service::AuthService;
use obscura_server::services::crypto_service::CryptoService;
use obscura_server::services::gateway::GatewayService;
use obscura_server::services::health_service::HealthService;
use obscura_server::services::key_service::KeyService;
use obscura_server::services::message_service::MessageService;
use obscura_server::services::notification::{DistributedNotificationService, NotificationService};
use obscura_server::services::push_token_service::PushTokenService;
use obscura_server::services::rate_limit_service::RateLimitService;
use obscura_server::workers::{AttachmentCleanupWorker, MessageCleanupWorker, PushNotificationWorker};
use obscura_server::{adapters, telemetry};
use std::net::SocketAddr;
use std::sync::Arc;
use tokio::sync::watch;
use tracing::Instrument;

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    let config = Config::load();
    let telemetry_guard = telemetry::init_telemetry(&config.telemetry)?;

    setup_panic_hook();

    let boot_span = tracing::info_span!("server_boot");
    let (api_listener, mgmt_listener, app, mgmt_app, shutdown_tx, shutdown_rx, worker_tasks) = async {
        // 1. Infrastructure Setup
        let pool = adapters::database::init_pool(&config.database_url).await?;
        run_migrations(&pool).await?;

        let (shutdown_tx, shutdown_rx) = watch::channel(false);
        spawn_signal_handler(shutdown_tx.clone());

        let pubsub = adapters::redis::RedisClient::new(
            &config.pubsub,
            config.notifications.global_channel_capacity,
            shutdown_rx.clone(),
        )
        .await?;

        let s3_client = init_s3_client(&config.storage).await;

        // 2. Service & Repository Wiring
        let (services, health_service, worker_tasks) =
            init_application(pool.clone(), pubsub, s3_client, &config, shutdown_rx.clone()).await?;

        // 3. Router & Listener Setup
        let app = api::app_router(config.clone(), services, shutdown_rx.clone());
        let mgmt_app = api::mgmt_router(MgmtState { health_service });

        let api_addr: SocketAddr = format!("{}:{}", config.server.host, config.server.port).parse()?;
        let mgmt_addr: SocketAddr = format!("{}:{}", config.server.host, config.server.mgmt_port).parse()?;

        tracing::info!(address = %api_addr, "listening");
        tracing::info!(address = %mgmt_addr, "management server listening");

        let api_listener = tokio::net::TcpListener::bind(api_addr).await?;
        let mgmt_listener = tokio::net::TcpListener::bind(mgmt_addr).await?;

        Ok::<
            (
                tokio::net::TcpListener,
                tokio::net::TcpListener,
                axum::Router,
                axum::Router,
                watch::Sender<bool>,
                watch::Receiver<bool>,
                Vec<tokio::task::JoinHandle<()>>,
            ),
            anyhow::Error,
        >((api_listener, mgmt_listener, app, mgmt_app, shutdown_tx, shutdown_rx, worker_tasks))
    }
    .instrument(boot_span)
    .await?;

    // 4. Server Execution
    let mut api_rx = shutdown_rx.clone();
    let api_server = axum::serve(api_listener, app.into_make_service_with_connect_info::<SocketAddr>())
        .with_graceful_shutdown(async move {
            let _ = api_rx.wait_for(|&s| s).await;
        });

    let mut mgmt_rx = shutdown_rx.clone();
    let mgmt_server = axum::serve(mgmt_listener, mgmt_app.into_make_service_with_connect_info::<SocketAddr>())
        .with_graceful_shutdown(async move {
            let _ = mgmt_rx.wait_for(|&s| s).await;
        });

    if let Err(e) = tokio::try_join!(api_server, mgmt_server) {
        tracing::error!(error = %e, "Server error");
    }

    // 5. Graceful Shutdown Orchestration
    let _ = shutdown_tx.send(true);
    tokio::select! {
        _ = async {
            futures::future::join_all(worker_tasks).await;
        } => {
            tracing::info!("Background tasks finished.");
        }
        _ = tokio::time::sleep(std::time::Duration::from_secs(config.server.shutdown_timeout_secs)) => {
            tracing::warn!("Timeout waiting for background tasks to finish.");
        }
    }

    telemetry_guard.shutdown();
    Ok(())
}

/// Initializes all repositories and services, and spawns background workers.
async fn init_application(
    pool: adapters::database::DbPool,
    pubsub: Arc<adapters::redis::RedisClient>,
    s3_client: aws_sdk_s3::Client,
    config: &Config,
    shutdown_rx: watch::Receiver<bool>,
) -> anyhow::Result<(ServiceContainer, HealthService, Vec<tokio::task::JoinHandle<()>>)> {
    let _span = tracing::info_span!("service_initialization").entered();

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
        Arc::new(adapters::redis::NotificationRepository::new(pubsub.clone(), &config.notifications));
    let notifier: Arc<dyn NotificationService> = Arc::new(
        DistributedNotificationService::new(notification_repo.clone(), &config.notifications, shutdown_rx.clone())
            .await?,
    );

    // Initialize Core Services
    let crypto_service = CryptoService::new();
    let key_service = KeyService::new(pool.clone(), key_repo, crypto_service, config.messaging.clone());
    let auth_service = AuthService::new(config.auth.clone(), pool.clone(), user_repo.clone(), refresh_repo);
    let message_service = MessageService::new(
        pool.clone(),
        message_repo.clone(),
        notifier.clone(),
        config.messaging.clone(),
        config.ttl_days,
    );
    let account_service = AccountService::new(
        pool.clone(),
        user_repo,
        message_repo.clone(),
        auth_service.clone(),
        key_service.clone(),
        notifier.clone(),
    );
    let gateway_service =
        GatewayService::new(message_service.clone(), key_service.clone(), notifier.clone(), config.websocket.clone());
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
        pubsub.clone(),
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
        Arc::new(adapters::push::fcm::FcmPushProvider),
        push_token_repo,
        &config.notifications,
    );
    tasks.push(tokio::spawn(async move { push_worker.run(shutdown_rx).await }));

    Ok((services, health_service, tasks))
}

async fn run_migrations(pool: &adapters::database::DbPool) -> anyhow::Result<()> {
    let _span = tracing::info_span!("database_migrations").entered();
    sqlx::migrate!().run(pool).await.map_err(Into::into)
}

async fn init_s3_client(config: &StorageConfig) -> aws_sdk_s3::Client {
    let _span = tracing::info_span!("storage_setup").entered();
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

fn spawn_signal_handler(shutdown_tx: watch::Sender<bool>) {
    tokio::spawn(async move {
        shutdown_signal().await;
        let _ = shutdown_tx.send(true);
    });
}

fn setup_panic_hook() {
    std::panic::set_hook(Box::new(|panic_info| {
        let payload = panic_info.payload();
        let msg = if let Some(s) = payload.downcast_ref::<&str>() {
            *s
        } else if let Some(s) = payload.downcast_ref::<String>() {
            s.as_str()
        } else {
            "Box<Any>"
        };

        let location = if let Some(location) = panic_info.location() {
            format!("{}:{}:{}", location.file(), location.line(), location.column())
        } else {
            "unknown".to_string()
        };

        tracing::error!(
            panic.message = %msg,
            panic.location = %location,
            "Application panicked"
        );
    }));
}

async fn shutdown_signal() {
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
        _ = ctrl_c => {},
        _ = terminate => {},
    }

    tracing::info!("Shutdown signal received, starting graceful shutdown...");
}
