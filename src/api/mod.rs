use crate::Services;
use crate::adapters::redis::SubmissionCache;
use crate::api::rate_limit::log_rate_limit_events;
use crate::config::Config;
use crate::services::account_service::AccountService;
use crate::services::attachment_service::AttachmentService;
use crate::services::auth_service::AuthService;
use crate::services::backup_service::BackupService;
use crate::services::gateway::GatewayService;
use crate::services::health_service::HealthService;
use crate::services::key_service::KeyService;
use crate::services::message_service::MessageService;
use crate::services::push_token_service::PushTokenService;
use crate::services::rate_limit_service::RateLimitService;
use axum::body::Body;
use axum::http::{Request, StatusCode};
use axum::{
    Router,
    middleware::from_fn_with_state,
    routing::{delete, get, head, post, put},
};
use std::sync::Arc;
use std::time::Duration;
use tower_governor::GovernorLayer;
use tower_governor::governor::GovernorConfigBuilder;
use tower_http::request_id::{PropagateRequestIdLayer, SetRequestIdLayer};
use tower_http::timeout::TimeoutLayer;
use tower_http::trace::TraceLayer;

pub mod attachments;
pub mod auth;
pub mod backup;
pub mod docs;
pub mod gateway;
pub mod health;
pub mod keys;
pub mod messages;
pub mod middleware;
pub mod push_tokens;
pub mod rate_limit;
pub mod schemas;

#[derive(Clone, Debug)]
pub(crate) struct AppState {
    pub(crate) config: Config,
    pub(crate) key_service: KeyService,
    pub(crate) attachment_service: AttachmentService,
    pub(crate) backup_service: BackupService,
    pub(crate) account_service: AccountService,
    pub(crate) auth_service: AuthService,
    pub(crate) message_service: MessageService,
    pub(crate) gateway_service: GatewayService,
    pub(crate) push_token_service: PushTokenService,
    pub(crate) rate_limit_service: RateLimitService,
    pub(crate) submission_cache: SubmissionCache,
    pub(crate) shutdown_rx: tokio::sync::watch::Receiver<bool>,
}

#[derive(Clone, Debug)]
pub struct MgmtState {
    pub health_service: HealthService,
}

/// Configures and returns the primary application router.
///
/// # Panics
/// Panics if the rate limiter configuration cannot be constructed.
#[allow(clippy::too_many_lines)]
pub fn app_router(config: &Config, services: Services, shutdown_rx: tokio::sync::watch::Receiver<bool>) -> Router {
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
        config: config.clone(),
        key_service: services.key_service,
        attachment_service: services.attachment_service,
        backup_service: services.backup_service,
        account_service: services.account_service,
        auth_service: services.auth_service,
        message_service: services.message_service,
        gateway_service: services.gateway_service,
        push_token_service: services.push_token_service,
        rate_limit_service: services.rate_limit_service,
        submission_cache: services.submission_cache,
        shutdown_rx,
    };

    let standard_timeout = TimeoutLayer::with_status_code(
        StatusCode::REQUEST_TIMEOUT,
        Duration::from_secs(config.server.request_timeout_secs),
    );

    // Sensitive routes with strict limits
    let auth_routes = Router::new()
        .route("/users", post(auth::register))
        .route("/sessions", post(auth::login))
        .route("/sessions", delete(auth::logout))
        .route("/sessions/refresh", post(auth::refresh))
        .layer(GovernorLayer::new(auth_conf))
        .layer(standard_timeout);

    // Standard routes
    let api_routes = Router::new()
        .route("/keys", post(keys::upload_keys))
        .route("/keys/{userId}", get(keys::get_pre_key_bundle))
        .route("/messages", post(messages::send_messages))
        .route("/gateway", get(gateway::websocket_handler))
        .route("/push-tokens", put(push_tokens::register_token))
        .layer(GovernorLayer::new(standard_conf))
        .layer(standard_timeout);

    // Storage routes (attachments and backups have their own service-level timeouts)
    let attachment_timeout = TimeoutLayer::with_status_code(
        StatusCode::REQUEST_TIMEOUT,
        Duration::from_secs(config.attachment.request_timeout_secs),
    );
    let backup_timeout = TimeoutLayer::with_status_code(
        StatusCode::REQUEST_TIMEOUT,
        Duration::from_secs(config.backup.request_timeout_secs),
    );

    let attachment_routes = Router::new()
        .route("/attachments", post(attachments::upload_attachment))
        .route("/attachments/{id}", get(attachments::download_attachment))
        .layer(attachment_timeout);

    let backup_routes = Router::new()
        .route("/backup", get(backup::download_backup))
        .route("/backup", post(backup::upload_backup))
        .route("/backup", head(backup::head_backup))
        .layer(backup_timeout);

    let storage_routes = attachment_routes.merge(backup_routes);

    Router::new()
        .route("/openapi.yaml", get(docs::openapi_yaml))
        .nest("/v1", auth_routes.merge(api_routes).merge(storage_routes))
        .layer(from_fn_with_state(state.clone(), log_rate_limit_events))
        .layer(PropagateRequestIdLayer::new(axum::http::HeaderName::from_static("x-request-id")))
        .layer(TimeoutLayer::with_status_code(
            StatusCode::REQUEST_TIMEOUT,
            Duration::from_secs(config.server.global_timeout_secs),
        ))
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
                .on_response(|response: &axum::http::Response<_>, latency: Duration, _span: &tracing::Span| {
                    let status = response.status();
                    tracing::Span::current().record("http.response.status_code", status.as_u16());

                    tracing::info!(
                        latency_ms = %latency.as_millis(),
                        status = %status.as_u16(),
                        "request completed"
                    );
                })
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
