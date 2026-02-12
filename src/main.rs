use obscura_server::api::{self, MgmtState, ServiceContainer};
use obscura_server::config::Config;
use obscura_server::services::account_service::AccountService;
use obscura_server::services::attachment_service::AttachmentService;
use obscura_server::services::auth_service::AuthService;
use obscura_server::services::crypto_service::CryptoService;
use obscura_server::services::gateway::GatewayService;
use obscura_server::services::health_service::HealthService;
use obscura_server::services::key_service::KeyService;
use obscura_server::services::message_service::MessageService;
use obscura_server::services::notification_service::{DistributedNotificationService, NotificationService};
use obscura_server::services::rate_limit_service::RateLimitService;
use obscura_server::storage::attachment_repo::AttachmentRepository;
use obscura_server::storage::key_repo::KeyRepository;
use obscura_server::storage::message_repo::MessageRepository;
use obscura_server::storage::refresh_token_repo::RefreshTokenRepository;
use obscura_server::storage::user_repo::UserRepository;
use obscura_server::{storage, telemetry};
use std::net::SocketAddr;
use std::sync::Arc;
use tokio::sync::watch;
use tracing::Instrument;

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    let config = Config::load();

    let telemetry_guard = telemetry::init_telemetry(&config.telemetry)?;

    let boot_span = tracing::info_span!("server_boot");

    let boot_result: anyhow::Result<_> = async {
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

        // Initialize pool
        let pool = storage::init_pool(&config.database_url).await?;

        // Run migrations
        {
            let _span = tracing::info_span!("database_migrations").entered();
            sqlx::migrate!().run(&pool).await?;
        }

        // Shutdown signaling
        let (shutdown_tx, shutdown_rx) = watch::channel(false);

        // Set up a task to listen for the OS signal and broadcast it internally
        let signal_tx = shutdown_tx.clone();
        tokio::spawn(async move {
            shutdown_signal().await;
            let _ = signal_tx.send(true);
        });

        // PubSub Setup (using Redis)
        let pubsub = {
            let _span = tracing::info_span!("pubsub_setup").entered();
            storage::redis::RedisClient::new(
                &config.pubsub,
                config.notifications.global_channel_capacity,
                shutdown_rx.clone(),
            )
            .await?
        };

        let notifier: Arc<dyn NotificationService> =
            Arc::new(DistributedNotificationService::new(pubsub.clone(), &config, shutdown_rx.clone()).await?);

        // Storage Setup
        let s3_client = {
            let _span = tracing::info_span!("storage_setup").entered();
            let region_provider = aws_config::Region::new(config.storage.region.clone());
            let mut config_loader = aws_config::defaults(aws_config::BehaviorVersion::latest()).region(region_provider);

            if let Some(ref endpoint) = config.storage.endpoint {
                config_loader = config_loader.endpoint_url(endpoint);
            }

            if let (Some(ak), Some(sk)) = (&config.storage.access_key, &config.storage.secret_key) {
                let creds = aws_credential_types::Credentials::new(ak.clone(), sk.clone(), None, None, "static");
                config_loader = config_loader.credentials_provider(creds);
            }

            let sdk_config = config_loader.load().await;
            let s3_config_builder =
                aws_sdk_s3::config::Builder::from(&sdk_config).force_path_style(config.storage.force_path_style);
            aws_sdk_s3::Client::from_conf(s3_config_builder.build())
        };

        // Initialize Repositories
        let (key_repo, message_repo, user_repo, refresh_repo, attachment_repo) = {
            let _span = tracing::info_span!("repository_initialization").entered();
            (
                KeyRepository::new(),
                MessageRepository::new(),
                UserRepository::new(),
                RefreshTokenRepository::new(),
                AttachmentRepository::new(),
            )
        };

        // Initialize Services
        let (
            key_service,
            attachment_service,
            account_service,
            message_service,
            gateway_service,
            rate_limit_service,
            health_service,
            auth_service,
        ) = {
            let _span = tracing::info_span!("service_initialization").entered();

            let crypto_service = CryptoService::new();

            let key_service = KeyService::new(pool.clone(), key_repo, crypto_service.clone(), config.messaging.clone());

            let attachment_service = AttachmentService::new(
                pool.clone(),
                attachment_repo,
                s3_client.clone(),
                config.storage.clone(),
                config.ttl_days,
            );

            let auth_service =
                AuthService::new(config.auth.clone(), pool.clone(), user_repo.clone(), refresh_repo.clone());

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

            let gateway_service = GatewayService::new(
                message_service.clone(),
                key_service.clone(),
                notifier.clone(),
                config.websocket.clone(),
            );

            let rate_limit_service = RateLimitService::new(config.server.trusted_proxies.clone());

            let health_service = HealthService::new(
                pool.clone(),
                s3_client.clone(),
                pubsub.clone(),
                config.storage.bucket.clone(),
                config.health.clone(),
            );

            (
                key_service,
                attachment_service,
                account_service,
                message_service,
                gateway_service,
                rate_limit_service,
                health_service,
                auth_service,
            )
        };

        // Start background tasks
        let message_cleanup = message_service.clone();
        let message_cleanup_rx = shutdown_rx.clone();
        let message_task = tokio::spawn(async move {
            message_cleanup.run_cleanup_loop(message_cleanup_rx).await;
        });

        let cleanup_service = attachment_service.clone();
        let attachment_cleanup_rx = shutdown_rx.clone();
        let attachment_task = tokio::spawn(async move {
            cleanup_service.run_cleanup_loop(attachment_cleanup_rx).await;
        });

        let services = ServiceContainer {
            pool: pool.clone(),
            key_service,
            attachment_service,
            account_service,
            auth_service,
            message_service,
            gateway_service,
            rate_limit_service,
        };

        let app = api::app_router(config.clone(), services, shutdown_rx.clone());

        let mgmt_state = MgmtState { health_service };
        let mgmt_app = api::mgmt_router(mgmt_state);

        let addr_str = format!("{}:{}", config.server.host, config.server.port);
        let addr: SocketAddr = addr_str.parse().expect("Invalid address format");
        let mgmt_addr_str = format!("{}:{}", config.server.host, config.server.mgmt_port);
        let mgmt_addr: SocketAddr = mgmt_addr_str.parse().expect("Invalid management address format");

        tracing::info!(address = %addr, "listening");
        tracing::info!(address = %mgmt_addr, "management server listening");

        let api_listener = tokio::net::TcpListener::bind(addr).await?;
        let mgmt_listener = tokio::net::TcpListener::bind(mgmt_addr).await?;

        Ok((api_listener, mgmt_listener, app, mgmt_app, shutdown_tx, shutdown_rx, message_task, attachment_task))
    }
    .instrument(boot_span)
    .await;

    let (api_listener, mgmt_listener, app, mgmt_app, shutdown_tx, shutdown_rx, message_task, attachment_task) =
        boot_result?;

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

    // Ensure all background tasks are signaled to shut down
    let _ = shutdown_tx.send(true);

    // Wait for tasks to finish (with timeout)
    tokio::select! {
        _ = async {
            let _ = tokio::join!(message_task, attachment_task);
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
