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
use obscura_server::{adapters, telemetry};
use std::net::SocketAddr;
use std::sync::Arc;
use tokio::sync::watch;
use tracing::Instrument;

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    let config = Config::load();
    let telemetry_guard = telemetry::init_telemetry(&config.telemetry)?;

    obscura_server::setup_panic_hook();

    let boot_span = tracing::info_span!("server_boot");
    let (api_listener, mgmt_listener, app, mgmt_app, shutdown_tx, shutdown_rx, worker_tasks) = async {
        // 1. Infrastructure Setup
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

        let s3_client = obscura_server::init_s3_client(&config.storage).await;

        // 2. Service & Repository Wiring
        let push_provider = Arc::new(adapters::push::fcm::FcmPushProvider);
        let (services, health_service, worker_tasks) = obscura_server::init_application(
            pool.clone(),
            pubsub,
            s3_client,
            push_provider,
            &config,
            shutdown_rx.clone(),
        )
        .await?;

        // 3. Router & Listener Setup
        let app = obscura_server::api::app_router(config.clone(), services, shutdown_rx.clone());
        let mgmt_app = obscura_server::api::mgmt_router(MgmtState { health_service });

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
