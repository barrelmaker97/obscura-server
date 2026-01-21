use crate::config::Config;
use crate::core::key_service::KeyService;
use crate::core::notification::Notifier;
use crate::storage::{DbPool, key_repo::KeyRepository, message_repo::MessageRepository};
use axum::{
    Router,
    body::Body,
    extract::State,
    http::{Request, StatusCode},
    middleware::{Next, from_fn_with_state},
    response::Response,
    routing::{get, post},
};
use std::sync::Arc;
use tower_governor::{GovernorLayer, governor::GovernorConfigBuilder};
use tracing::warn;

pub mod attachments;
pub mod auth;
pub mod docs;
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
    pub extractor: rate_limit::IpKeyExtractor,
    pub s3_client: aws_sdk_s3::Client,
    pub key_service: KeyService,
}

pub fn app_router(pool: DbPool, config: Config, notifier: Arc<dyn Notifier>, s3_client: aws_sdk_s3::Client) -> Router {
    let extractor = rate_limit::IpKeyExtractor::new(config.server.trusted_proxies.clone());

    // Initialize Services
    let key_repo = KeyRepository::new(pool.clone());
    let message_repo = MessageRepository::new(pool.clone());
    let key_service = KeyService::new(
        pool.clone(),
        key_repo,
        message_repo,
        notifier.clone(),
        config.clone(),
    );

    // Standard Tier: For general API usage
    let std_interval_ns = 1_000_000_000 / config.rate_limit.per_second.max(1);
    let standard_conf = Arc::new(
        GovernorConfigBuilder::default()
            .per_nanosecond(std_interval_ns as u64)
            .burst_size(config.rate_limit.burst)
            .key_extractor(extractor.clone())
            .finish()
            .unwrap(),
    );

    // Auth Tier: Stricter limits for expensive/sensitive registration & login
    let auth_interval_ns = 1_000_000_000 / config.rate_limit.auth_per_second.max(1);
    let auth_conf = Arc::new(
        GovernorConfigBuilder::default()
            .per_nanosecond(auth_interval_ns as u64)
            .burst_size(config.rate_limit.auth_burst)
            .key_extractor(extractor.clone())
            .finish()
            .unwrap(),
    );

    let state = AppState { pool, config, notifier, extractor, s3_client, key_service };

    // Sensitive routes with strict limits
    let auth_routes = Router::new()
        .route("/users", post(auth::register))
        .route("/sessions", post(auth::login))
        .route("/sessions", axum::routing::delete(auth::logout))
        .route("/sessions/refresh", post(auth::refresh))
        .layer(GovernorLayer::new(auth_conf));

    // Standard routes
    let api_routes = Router::new()
        .route("/keys", post(keys::upload_keys))
        .route("/keys/{userId}", get(keys::get_pre_key_bundle))
        .route("/messages/{recipientId}", post(messages::send_message))
        .route("/gateway", get(gateway::websocket_handler))
        .route("/attachments", post(attachments::upload_attachment))
        .route("/attachments/{id}", get(attachments::download_attachment))
        .layer(GovernorLayer::new(standard_conf));

    Router::new()
        .route("/openapi.yaml", get(docs::openapi_yaml))
        .nest("/v1", auth_routes.merge(api_routes))
        .layer(from_fn_with_state(state.clone(), log_rate_limit_events))
        .with_state(state)
}

async fn log_rate_limit_events(State(state): State<AppState>, req: Request<Body>, next: Next) -> Response {
    let method = req.method().clone();

    // We must extract information BEFORE calling next.run(req), as that consumes the request.
    let path = req.uri().path().to_string();
    let headers = req.headers().clone();
    let peer_addr = req.extensions().get::<axum::extract::ConnectInfo<std::net::SocketAddr>>().map(|info| info.0.ip());

    let mut response = next.run(req).await;

    if response.status() == StatusCode::TOO_MANY_REQUESTS {
        // Use the exact same secure logic as the rate limiter to identify the client IP.
        let ip = peer_addr
            .map(|addr| state.extractor.identify_client_ip(&headers, addr).to_string())
            .unwrap_or_else(|| "unknown".into());

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
