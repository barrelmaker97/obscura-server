use obscura_server::{api, config::Config, core::notification::InMemoryNotifier, storage};
use std::net::SocketAddr;
use std::sync::Arc;
use tracing_subscriber::{layer::SubscriberExt, util::SubscriberInitExt};

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    tracing_subscriber::registry()
        .with(tracing_subscriber::EnvFilter::new(std::env::var("RUST_LOG").unwrap_or_else(|_| "debug".into())))
        .with(tracing_subscriber::fmt::layer())
        .init();

    let config = Config::load();

    // Initialize pool
    let pool = storage::init_pool(&config.database_url).await?;

    // Run migrations
    tracing::info!("Running migrations...");
    sqlx::migrate!().run(&pool).await?;
    tracing::info!("Migrations complete.");

    // Start background tasks
    let message_service = obscura_server::core::message_service::MessageService::new(
        obscura_server::storage::message_repo::MessageRepository::new(pool.clone()),
        config.clone(),
    );
    tokio::spawn(async move {
        message_service.run_cleanup_loop().await;
    });

    // S3 Setup
    let region_provider = aws_config::Region::new(config.s3_region.clone());
    let mut config_loader = aws_config::defaults(aws_config::BehaviorVersion::latest()).region(region_provider);

    if let Some(ref endpoint) = config.s3_endpoint {
        config_loader = config_loader.endpoint_url(endpoint);
    }

    if let (Some(ak), Some(sk)) = (&config.s3_access_key, &config.s3_secret_key) {
        let creds = aws_credential_types::Credentials::new(ak.clone(), sk.clone(), None, None, "static");
        config_loader = config_loader.credentials_provider(creds);
    }

    let sdk_config = config_loader.load().await;
    let s3_config_builder = aws_sdk_s3::config::Builder::from(&sdk_config).force_path_style(config.s3_force_path_style);
    let s3_client = aws_sdk_s3::Client::from_conf(s3_config_builder.build());

    let attachment_service = obscura_server::core::attachment_service::AttachmentCleanupService::new(
        pool.clone(),
        s3_client.clone(),
        config.clone(),
    );
    tokio::spawn(async move {
        attachment_service.run_cleanup_loop().await;
    });

    let notifier = Arc::new(InMemoryNotifier::new(config.clone()));
    let app = api::app_router(pool, config.clone(), notifier, s3_client);

    let addr_str = format!("{}:{}", config.server_host, config.server_port);
    let addr: SocketAddr = addr_str.parse().expect("Invalid address format");
    tracing::info!("listening on {}", addr);
    let listener = tokio::net::TcpListener::bind(addr).await?;
    axum::serve(listener, app.into_make_service_with_connect_info::<SocketAddr>()).await?;

    Ok(())
}
