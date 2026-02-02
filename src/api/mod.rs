use crate::api::rate_limit::{IpKeyExtractor, log_rate_limit_events};
use crate::config::{Config, HealthConfig};
use crate::core::account_service::AccountService;
use crate::core::attachment_service::AttachmentService;
use crate::core::key_service::KeyService;
use crate::core::message_service::MessageService;
use crate::core::notification::Notifier;
use crate::storage::{
    DbPool, attachment_repo::AttachmentRepository, key_repo::KeyRepository, message_repo::MessageRepository,
    refresh_token_repo::RefreshTokenRepository, user_repo::UserRepository,
};
use axum::body::Body;
use axum::extract::ConnectInfo;
use axum::http::Request;
use axum::{
    Router,
    middleware::from_fn,
    routing::{get, post},
};
use std::net::SocketAddr;
use std::sync::Arc;
use tower_governor::{GovernorLayer, governor::GovernorConfigBuilder};
use tower_http::request_id::{PropagateRequestIdLayer, SetRequestIdLayer};
use tower_http::trace::{DefaultOnResponse, TraceLayer};
use tracing::Level;

pub mod attachments;
pub mod auth;
pub mod docs;
pub mod gateway;
pub mod health;
pub mod keys;
pub mod messages;
pub mod middleware;
pub mod rate_limit;

#[derive(Clone)]
pub struct AppState {
    pub pool: DbPool,
    pub config: Config,
    pub notifier: Arc<dyn Notifier>,
    pub extractor: IpKeyExtractor,
    pub s3_client: aws_sdk_s3::Client,
    pub key_service: KeyService,
    pub attachment_service: AttachmentService,
    pub account_service: AccountService,
    pub message_service: MessageService,
    pub shutdown_rx: tokio::sync::watch::Receiver<bool>,
}

#[derive(Clone)]
pub struct MgmtState {
    pub pool: DbPool,
    pub health_config: HealthConfig,
    pub s3_bucket: String,
    pub s3_client: aws_sdk_s3::Client,
}

pub fn app_router(
    pool: DbPool,
    config: Config,
    notifier: Arc<dyn Notifier>,
    s3_client: aws_sdk_s3::Client,
    shutdown_rx: tokio::sync::watch::Receiver<bool>,
) -> Router {
    let extractor = IpKeyExtractor::new(config.server.trusted_proxies.clone());

    // Initialize Repositories
    let key_repo = KeyRepository::new();
    let message_repo = MessageRepository::new();
    let user_repo = UserRepository::new();
    let refresh_repo = RefreshTokenRepository::new();
    let attachment_repo = AttachmentRepository::new();

    // Initialize Services
    let key_service =
        KeyService::new(pool.clone(), key_repo, message_repo.clone(), notifier.clone(), config.messaging.clone());
    let attachment_service =
        AttachmentService::new(pool.clone(), attachment_repo, s3_client.clone(), config.s3.clone(), config.ttl_days);
    let account_service =
        AccountService::new(pool.clone(), config.auth.clone(), key_service.clone(), user_repo, refresh_repo);
    let message_service = MessageService::new(
        pool.clone(),
        message_repo.clone(),
        notifier.clone(),
        config.messaging.clone(),
        config.ttl_days,
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

    let trace_extractor = extractor.clone();

    let state = AppState {
        pool,
        config,
        notifier,
        extractor,
        s3_client,
        key_service,
        attachment_service,
        account_service,
        message_service,
        shutdown_rx,
    };

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
        .layer(from_fn(log_rate_limit_events))
        .layer(PropagateRequestIdLayer::new(
            axum::http::HeaderName::from_static("x-request-id"),
        ))
        .layer(
            TraceLayer::new_for_http()
                .make_span_with(move |request: &Request<Body>| {
                    let peer_addr = request.extensions().get::<ConnectInfo<SocketAddr>>().map(|info| info.0.ip());

                    let client_ip = peer_addr
                        .map(|ip| trace_extractor.identify_client_ip(request.headers(), ip).to_string())
                        .unwrap_or_else(|| "unknown".to_string());

                    let request_id = request
                        .extensions()
                        .get::<tower_http::request_id::RequestId>()
                        .map(|id| id.header_value().to_str().unwrap_or_default())
                        .unwrap_or_default()
                        .to_string();

                    tracing::info_span!(
                        "request",
                        request_id = %request_id,
                        method = %request.method(),
                        path = %request.uri().path(),
                        client_ip = %client_ip,
                        user_id = tracing::field::Empty,
                    )
                })
                .on_response(DefaultOnResponse::new().level(Level::INFO)),
        )
        .layer(SetRequestIdLayer::new(
            axum::http::HeaderName::from_static("x-request-id"),
            middleware::MakeRequestUuidOrHeader,
        ))
        .with_state(state)
}

pub fn mgmt_router(state: MgmtState) -> Router {
    Router::new()
        .route("/livez", get(health::livez))
        .route("/readyz", get(health::readyz))
        .route("/metrics", get(health::metrics))
        .with_state(state)
}
