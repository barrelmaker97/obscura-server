use axum::{Router, routing::{get, post, put}};
use crate::storage::DbPool;
use crate::config::Config;

pub mod auth;
pub mod keys;
pub mod messages;
pub mod gateway;
pub mod middleware;

#[derive(Clone)]
pub struct AppState {
    pub pool: DbPool,
    pub config: Config,
}

pub fn app_router(pool: DbPool, config: Config) -> Router {
    let state = AppState { pool, config };
    
    Router::new()
        .route("/v1/accounts", post(auth::register))
        .route("/v1/keys", put(keys::upload_keys))
        .route("/v1/keys/{userId}", get(keys::get_pre_key_bundle))
        .route("/v1/messages/{destinationDeviceId}", post(messages::send_message))
        .route("/v1/gateway", get(gateway::websocket_handler))
        .with_state(state)
}