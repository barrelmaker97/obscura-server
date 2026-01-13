use axum::{Router, routing::{get, post, put}};
use crate::storage::DbPool;
use crate::config::Config;
use crate::core::notification::Notifier;
use std::sync::Arc;
use tower_governor::{GovernorLayer, governor::GovernorConfigBuilder};

pub mod auth;
pub mod keys;
pub mod messages;
pub mod gateway;
pub mod middleware;

#[derive(Clone)]
pub struct AppState {
    pub pool: DbPool,
    pub config: Config,
    pub notifier: Arc<dyn Notifier>,
}

pub fn app_router(pool: DbPool, config: Config, notifier: Arc<dyn Notifier>) -> Router {
    let governor_conf = Arc::new(
        GovernorConfigBuilder::default()
            .per_second(config.rate_limit_per_second as u64)
            .burst_size(config.rate_limit_burst)
            .finish()
            .unwrap()
    );

    let state = AppState { pool, config, notifier };

    Router::new()
        .route("/v1/accounts", post(auth::register))
        .route("/v1/sessions", post(auth::login))
        .route("/v1/keys", put(keys::upload_keys))
        .route("/v1/keys/{userId}", get(keys::get_pre_key_bundle))
        .route("/v1/messages/{destinationDeviceId}", post(messages::send_message))
        .route("/v1/gateway", get(gateway::websocket_handler))
        .layer(GovernorLayer::new(governor_conf))
        .with_state(state)
}
