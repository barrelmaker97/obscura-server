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
    let extractor = rate_limit::IpKeyExtractor::new(&config.trusted_proxies);

    // Standard Tier: For general API usage
    let std_interval_ns = 1_000_000_000 / config.rate_limit_per_second.max(1);
    let standard_conf = Arc::new(
        GovernorConfigBuilder::default()
            .per_nanosecond(std_interval_ns as u64)
            .burst_size(config.rate_limit_burst)
            .key_extractor(extractor.clone())
            .finish()
            .unwrap(),
    );

    // Auth Tier: Stricter limits for expensive/sensitive registration & login
    let auth_interval_ns = 1_000_000_000 / config.auth_rate_limit_per_second.max(1);
    let auth_conf = Arc::new(
        GovernorConfigBuilder::default()
            .per_nanosecond(auth_interval_ns as u64)
            .burst_size(config.auth_rate_limit_burst)
            .key_extractor(extractor)
            .finish()
            .unwrap(),
    );

    let state = AppState { pool, config, notifier };

    // Sensitive routes with strict limits
    let auth_routes = Router::new()
        .route("/accounts", post(auth::register))
        .route("/sessions", post(auth::login))
        .layer(GovernorLayer::new(auth_conf));

    // Standard routes
    let api_routes = Router::new()
        .route("/keys", post(keys::upload_keys))
        .route("/keys/{userId}", get(keys::get_pre_key_bundle))
        .route("/messages/{recipientId}", post(messages::send_message))
        .route("/gateway", get(gateway::websocket_handler))
        .layer(GovernorLayer::new(standard_conf));

    Router::new().nest("/v1", auth_routes.merge(api_routes)).layer(from_fn(log_rate_limit_events)).with_state(state)
}

async fn log_rate_limit_events(req: Request<Body>, next: Next) -> Response {
    let method = req.method().clone();

    // We must extract the path and IP information BEFORE calling next.run(req),
    // as that consumes the request object.
    let path = req.uri().path().to_string();

    let ip_header = req
        .headers()
        .get("x-forwarded-for")
        .and_then(|v| v.to_str().ok())
        .and_then(|s| s.split(',').next())
        .map(|s| s.to_string());

    let connect_info = req.extensions().get::<axum::extract::ConnectInfo<std::net::SocketAddr>>().cloned();

    let mut response = next.run(req).await;

    if response.status() == StatusCode::TOO_MANY_REQUESTS {
        let ip = ip_header
            .unwrap_or_else(|| connect_info.map(|info| info.0.ip().to_string()).unwrap_or_else(|| "unknown".into()));

        warn!("Rate limit hit: client_ip={}, method={}, path={}", ip, method, path);

        // Map the internal x-ratelimit-after to the standard Retry-After header
        // for better compatibility with standard HTTP clients.
        if let Some(after) = response.headers().get("x-ratelimit-after") {
            let after = after.clone();
            response.headers_mut().insert("retry-after", after);
        }
    }

    response
}
