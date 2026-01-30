use obscura_server::{api, config::Config, core::notification::InMemoryNotifier, storage};
use std::net::SocketAddr;
use std::sync::Arc;
use tokio::sync::watch;
use tracing_subscriber::{layer::SubscriberExt, util::SubscriberInitExt};

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    tracing_subscriber::registry()
        .with(tracing_subscriber::EnvFilter::new(std::env::var("RUST_LOG").unwrap_or_else(|_| "info".into())))
        .with(tracing_subscriber::fmt::layer())
        .init();

    let config = Config::load();

    // Initialize pool
    let pool = storage::init_pool(&config.database_url).await?;

    // Run migrations
    tracing::info!("Running migrations...");
    sqlx::migrate!().run(&pool).await?;
    // Shutdown signaling
    let (shutdown_tx, shutdown_rx) = watch::channel(false);

    let notifier = Arc::new(InMemoryNotifier::new(config.clone(), shutdown_rx.clone()));

    // Start background tasks
    let message_service = obscura_server::core::message_service::MessageService::new(
        pool.clone(),
        obscura_server::storage::message_repo::MessageRepository::new(),
        notifier.clone(),
        config.messaging.clone(),
        config.ttl_days,
    );
    let message_cleanup = message_service.clone();
    let message_cleanup_rx = shutdown_rx.clone();
    let message_task = tokio::spawn(async move {
        message_cleanup.run_cleanup_loop(message_cleanup_rx).await;
    });

    // S3 Setup
    let region_provider = aws_config::Region::new(config.s3.region.clone());
    let mut config_loader = aws_config::defaults(aws_config::BehaviorVersion::latest()).region(region_provider);

    if let Some(ref endpoint) = config.s3.endpoint {
        config_loader = config_loader.endpoint_url(endpoint);
    }

    if let (Some(ak), Some(sk)) = (&config.s3.access_key, &config.s3.secret_key) {
        let creds = aws_credential_types::Credentials::new(ak.clone(), sk.clone(), None, None, "static");
        config_loader = config_loader.credentials_provider(creds);
    }

    let sdk_config = config_loader.load().await;
    let s3_config_builder = aws_sdk_s3::config::Builder::from(&sdk_config).force_path_style(config.s3.force_path_style);
    let s3_client = aws_sdk_s3::Client::from_conf(s3_config_builder.build());

    let attachment_service = obscura_server::core::attachment_service::AttachmentService::new(
        pool.clone(),
        obscura_server::storage::attachment_repo::AttachmentRepository::new(),
        s3_client.clone(),
        config.s3.clone(),
        config.ttl_days,
    );
    let cleanup_service = attachment_service.clone();
    let attachment_cleanup_rx = shutdown_rx.clone();
    let attachment_task = tokio::spawn(async move {
        cleanup_service.run_cleanup_loop(attachment_cleanup_rx).await;
    });

    let app = api::app_router(pool.clone(), config.clone(), notifier.clone(), s3_client.clone());
    let mgmt_state =
        obscura_server::api::MgmtState { pool: pool.clone(), config: config.clone(), s3_client: s3_client.clone() };
    let mgmt_app = api::mgmt_router(mgmt_state);

    let addr_str = format!("{}:{}", config.server.host, config.server.port);
    let addr: SocketAddr = addr_str.parse().expect("Invalid address format");
    let mgmt_addr_str = format!("{}:{}", config.server.host, config.server.mgmt_port);
    let mgmt_addr: SocketAddr = mgmt_addr_str.parse().expect("Invalid management address format");

    tracing::info!("listening on {}", addr);
    tracing::info!("management server listening on {}", mgmt_addr);

    let api_listener = tokio::net::TcpListener::bind(addr).await?;
    let mgmt_listener = tokio::net::TcpListener::bind(mgmt_addr).await?;

    let api_server = axum::serve(api_listener, app.into_make_service_with_connect_info::<SocketAddr>())
        .with_graceful_shutdown(shutdown_signal());

    let mgmt_server = axum::serve(mgmt_listener, mgmt_app.into_make_service_with_connect_info::<SocketAddr>())
        .with_graceful_shutdown(shutdown_signal());

    tokio::select! {
        res = api_server => {
            if let Err(e) = res {
                tracing::error!("API server error: {}", e);
            }
        }
        res = mgmt_server => {
            if let Err(e) = res {
                tracing::error!("Management server error: {}", e);
            }
        }
    }

    // Signal background tasks to shut down
    tracing::info!("Signaling background tasks to shut down...");
    let _ = shutdown_tx.send(true);

    // Wait for tasks to finish (with timeout)
    tokio::select! {
        _ = async {
            let _ = tokio::join!(message_task, attachment_task);
        } => {
            tracing::info!("Background tasks finished.");
        }
        _ = tokio::time::sleep(std::time::Duration::from_secs(5)) => {
            tracing::warn!("Timeout waiting for background tasks to finish.");
        }
    }

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
