use crate::config::Config;
use crate::core::notification::Notifier;
use crate::storage::DbPool;
use axum::{
    Router,
    body::Body,
    http::{Request, StatusCode},
    middleware::{Next, from_fn},
    response::Response,
    routing::{get, post},
};
use std::sync::Arc;
use tower_governor::{GovernorLayer, governor::GovernorConfigBuilder};
use tracing::warn;

pub mod auth;
pub mod gateway;
pub mod keys;
pub mod messages;
pub mod middleware;
pub mod rate_limit;

#[derive(Clone)]
pub struct AppState {
    pub pool: DbPool,
    pub config: Config,
    pub notifier: Arc<dyn Notifier>,
}

pub fn app_router(pool: DbPool, config: Config, notifier: Arc<dyn Notifier>) -> Router {
    let interval_ms = 1000 / config.rate_limit_per_second.max(1);
    let governor_conf = Arc::new(
        GovernorConfigBuilder::default()
            .per_millisecond(interval_ms as u64)
            .burst_size(config.rate_limit_burst)
            .key_extractor(rate_limit::IpKeyExtractor)
            .finish()
            .unwrap(),
    );

    let state = AppState { pool, config, notifier };

    Router::new()
        .route("/v1/accounts", post(auth::register))
        .route("/v1/sessions", post(auth::login))
        .route("/v1/keys", post(keys::upload_keys))
        .route("/v1/keys/{userId}", get(keys::get_pre_key_bundle))
        .route("/v1/messages/{recipientId}", post(messages::send_message))
        .route("/v1/gateway", get(gateway::websocket_handler))
        .layer(from_fn(log_rate_limit_events))
        .layer(GovernorLayer::new(governor_conf))
        .with_state(state)
}

async fn log_rate_limit_events(req: Request<Body>, next: Next) -> Response {
    let method = req.method().clone();
    let path = req.uri().path().to_string(); // Simple allocation, but safe. 
    // Optimization: We could use a Cow or only allocate on failure, 
    // but req is consumed by next.run(req).
    
    let ip_header = req.headers().get("x-forwarded-for")
        .and_then(|v| v.to_str().ok())
        .and_then(|s| s.split(',').next())
        .map(|s| s.to_string());
    
    let connect_info = req.extensions().get::<axum::extract::ConnectInfo<std::net::SocketAddr>>().cloned();

    let mut response = next.run(req).await;

    if response.status() == StatusCode::TOO_MANY_REQUESTS {
        let ip = ip_header.unwrap_or_else(|| {
            connect_info
                .map(|info| info.0.ip().to_string())
                .unwrap_or_else(|| "unknown".into())
        });

        warn!(
            "Rate limit hit: client_ip={}, method={}, path={}",
            ip, method, path
        );

        if let Some(after) = response.headers().get("x-ratelimit-after") {
            let after = after.clone();
            response.headers_mut().insert("retry-after", after);
        }
    }

    response
}
