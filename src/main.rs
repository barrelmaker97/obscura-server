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

use obscura_server::api::MgmtState;
use obscura_server::config::Config;
use obscura_server::{AppBuilder, adapters, telemetry};
use std::net::SocketAddr;
use std::sync::Arc;
use tokio::sync::watch;
use tracing::Instrument;

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    let config = Config::load();
    let telemetry_guard = telemetry::init_telemetry(&config.telemetry)?;

    obscura_server::setup_panic_hook();

    let boot_span = tracing::info_span!("boot_server");
    let (api_listener, mgmt_listener, app_router, mgmt_app, shutdown_tx, shutdown_rx, workers) = async {
        // Phase 1: Infrastructure Setup (Resources)
        let pool = adapters::database::init_pool(&config.database_url).await?;
        obscura_server::run_migrations(&pool).await?;

        let (shutdown_tx, shutdown_rx) = watch::channel(false);
        obscura_server::spawn_signal_handler(shutdown_tx.clone());

        let pubsub = adapters::redis::RedisClient::new(
            &config.pubsub,
            config.notifications.global_channel_capacity,
            shutdown_rx.clone(),
        )
        .await?;

        let s3_client = obscura_server::initialize_s3_client(&config.storage).await;

        // Phase 2: Component Wiring (Pure logic, no side effects)
        let push_provider = Arc::new(adapters::push::fcm::FcmPushProvider);
        let app = AppBuilder::new(config.clone())
            .with_database(pool)
            .with_pubsub(pubsub)
            .with_s3(s3_client)
            .with_push_provider(push_provider)
            .with_shutdown_rx(shutdown_rx.clone())
            .build()
            .await?;

        // Phase 3: Runtime Setup (Listeners and Routers)
        let app_router = obscura_server::api::app_router(config.clone(), app.services, shutdown_rx.clone());
        let mgmt_app = obscura_server::api::mgmt_router(MgmtState { health_service: app.health_service });

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
                obscura_server::Workers,
            ),
            anyhow::Error,
        >((api_listener, mgmt_listener, app_router, mgmt_app, shutdown_tx, shutdown_rx, app.workers))
    }
    .instrument(boot_span)
    .await?;

    // Phase 4: Start Runtime (Explicit Spawning and Listening)
    let worker_tasks = workers.spawn_all(shutdown_rx.clone());

    let mut api_rx = shutdown_rx.clone();
    let api_server = axum::serve(api_listener, app_router.into_make_service_with_connect_info::<SocketAddr>())
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

    // Phase 5: Graceful Shutdown Orchestration
    let _ = shutdown_tx.send(true);
    tokio::select! {
        () = async {
            futures::future::join_all(worker_tasks).await;
        } => {
            tracing::info!("Background tasks finished.");
        }
        () = tokio::time::sleep(std::time::Duration::from_secs(config.server.shutdown_timeout_secs)) => {
            tracing::warn!("Timeout waiting for background tasks to finish.");
        }
    }

    telemetry_guard.shutdown();
    Ok(())
}
