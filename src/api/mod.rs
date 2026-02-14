use crate::adapters::database::DbPool;
use crate::api::rate_limit::log_rate_limit_events;
use crate::config::Config;
use crate::services::account_service::AccountService;
use crate::services::attachment_service::AttachmentService;
use crate::services::auth_service::AuthService;
use crate::services::gateway::GatewayService;
use crate::services::health_service::HealthService;
use crate::services::key_service::KeyService;
use crate::services::message_service::MessageService;
use crate::services::notification::NotificationService;
use crate::services::push_token_service::PushTokenService;
use crate::services::rate_limit_service::RateLimitService;
use axum::body::Body;
use axum::http::Request;
use axum::{
    Router,
    middleware::from_fn_with_state,
    routing::{delete, get, post, put},
};
use std::sync::Arc;
use tower_governor::GovernorLayer;
use tower_governor::governor::GovernorConfigBuilder;
use tower_http::request_id::{PropagateRequestIdLayer, SetRequestIdLayer};
use tower_http::trace::TraceLayer;

pub mod attachments;
pub mod auth;
pub mod docs;
pub mod gateway;
pub mod health;
pub mod keys;
pub mod messages;
pub mod middleware;
pub mod notifications;
pub mod rate_limit;
pub mod schemas;

#[derive(Clone, Debug)]
pub struct AppState {
    pub config: Config,
    pub key_service: KeyService,
    pub attachment_service: AttachmentService,
    pub account_service: AccountService,
    pub auth_service: AuthService,
    pub message_service: MessageService,
    pub gateway_service: GatewayService,
    pub notification_service: Arc<dyn NotificationService>,
    pub push_token_service: PushTokenService,
    pub rate_limit_service: RateLimitService,
    pub shutdown_rx: tokio::sync::watch::Receiver<bool>,
}

#[derive(Clone, Debug)]
pub struct MgmtState {
    pub health_service: HealthService,
}

#[derive(Debug)]
pub struct ServiceContainer {
    pub pool: DbPool,
    pub key_service: KeyService,
    pub attachment_service: AttachmentService,
    pub account_service: AccountService,
    pub auth_service: AuthService,
    pub message_service: MessageService,
    pub gateway_service: GatewayService,
    pub notification_service: Arc<dyn NotificationService>,
    pub push_token_service: PushTokenService,
    pub rate_limit_service: RateLimitService,
}

/// Configures and returns the primary application router.
///
/// # Panics
/// Panics if the rate limiter configuration cannot be constructed.
pub fn app_router(
    config: Config,
    services: ServiceContainer,
    shutdown_rx: tokio::sync::watch::Receiver<bool>,
) -> Router {
    let std_interval_ns = 1_000_000_000 / config.rate_limit.per_second.max(1);
    let standard_conf = Arc::new(
        GovernorConfigBuilder::default()
            .per_nanosecond(u64::from(std_interval_ns))
            .burst_size(config.rate_limit.burst)
            .key_extractor(services.rate_limit_service.extractor.clone())
            .finish()
            .expect("Failed to build standard rate limiter config"),
    );

    // Auth Tier: Stricter limits for expensive/sensitive registration & login
    let auth_interval_ns = 1_000_000_000 / config.rate_limit.auth_per_second.max(1);
    let auth_conf = Arc::new(
        GovernorConfigBuilder::default()
            .per_nanosecond(u64::from(auth_interval_ns))
            .burst_size(config.rate_limit.auth_burst)
            .key_extractor(services.rate_limit_service.extractor.clone())
            .finish()
            .expect("Failed to build auth rate limiter config"),
    );

    let state = AppState {
        config,
        key_service: services.key_service,
        attachment_service: services.attachment_service,
        account_service: services.account_service,
        auth_service: services.auth_service,
        message_service: services.message_service,
        gateway_service: services.gateway_service,
        notification_service: services.notification_service,
        push_token_service: services.push_token_service,
        rate_limit_service: services.rate_limit_service,
        shutdown_rx,
    };

    // Sensitive routes with strict limits
    let auth_routes = Router::new()
        .route("/users", post(auth::register))
        .route("/sessions", post(auth::login))
        .route("/sessions", delete(auth::logout))
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
        .route("/push/token", put(notifications::register_token))
        .layer(GovernorLayer::new(standard_conf));

    Router::new()
        .route("/openapi.yaml", get(docs::openapi_yaml))
        .nest("/v1", auth_routes.merge(api_routes))
        .layer(from_fn_with_state(state.clone(), log_rate_limit_events))
        .layer(PropagateRequestIdLayer::new(axum::http::HeaderName::from_static("x-request-id")))
        .layer(
            TraceLayer::new_for_http()
                .make_span_with(move |request: &Request<Body>| {
                    let request_id = request
                        .extensions()
                        .get::<tower_http::request_id::RequestId>()
                        .map(|id| id.header_value().to_str().unwrap_or_default())
                        .unwrap_or_default()
                        .to_string();

                    tracing::info_span!(
                        "request",
                        "request_id" = %request_id,
                        "http.request.method" = %request.method(),
                        "url.path" = %request.uri().path(),
                        "http.response.status_code" = tracing::field::Empty,
                        "otel.kind" = "server",
                        "user_id" = tracing::field::Empty,
                    )
                })
                .on_response(
                    |response: &axum::http::Response<_>, latency: std::time::Duration, _span: &tracing::Span| {
                        let status = response.status();
                        tracing::Span::current().record("http.response.status_code", status.as_u16());

                        tracing::info!(
                            latency_ms = %latency.as_millis(),
                            status = %status.as_u16(),
                            "request completed"
                        );
                    },
                )
                .on_failure(|error, _latency, _span: &tracing::Span| {
                    tracing::error!(error = %error, "request failed");
                }),
        )
        .layer(SetRequestIdLayer::new(
            axum::http::HeaderName::from_static("x-request-id"),
            middleware::MakeRequestUuidOrHeader,
        ))
        .with_state(state)
}

pub fn mgmt_router(state: MgmtState) -> Router {
    Router::new().route("/livez", get(health::livez)).route("/readyz", get(health::readyz)).with_state(state)
}
