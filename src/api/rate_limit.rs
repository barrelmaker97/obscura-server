use crate::api::AppState;
use axum::body::Body;
use axum::extract::State;
use axum::http::{Request, StatusCode};
use axum::middleware::Next;
use axum::response::Response;

/// Middleware to log rate limit events and add Retry-After headers.
///
/// # Panics
/// Panics if the `x-ratelimit-after` header value cannot be parsed as a string.
pub async fn log_rate_limit_events(State(state): State<AppState>, req: Request<Body>, next: Next) -> Response {
    let mut response = next.run(req).await;

    let status = response.status();
    let ratelimit_after = if status == StatusCode::TOO_MANY_REQUESTS {
        response.headers().get("x-ratelimit-after").and_then(|v| v.to_str().ok().map(ToString::to_string))
    } else {
        None
    };

    state.rate_limit_service.log_decision(status, ratelimit_after.clone());

    if status == StatusCode::TOO_MANY_REQUESTS
        && let Some(after) = ratelimit_after
    {
        response.headers_mut().insert("retry-after", after.parse().expect("Failed to parse retry-after header value"));
    }

    response
}
