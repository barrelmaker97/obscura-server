use crate::api::AppState;
use crate::domain::auth::Jwt;
use axum::{
    extract::{
        Query, State,
        ws::WebSocketUpgrade,
    },
    http::Extensions,
    response::IntoResponse,
};
use serde::Deserialize;
use tower_http::request_id::RequestId;

#[derive(Deserialize)]
pub struct WsParams {
    token: String,
}

pub async fn websocket_handler(
    ws: WebSocketUpgrade,
    Query(params): Query<WsParams>,
    extensions: Extensions,
    State(state): State<AppState>,
) -> impl IntoResponse {
    let request_id = extensions
        .get::<RequestId>()
        .map_or_else(|| "unknown".to_string(), |id| id.header_value().to_str().unwrap_or_default().to_string());

    let jwt = Jwt::new(params.token);
    match state.auth_service.verify_token(jwt) {
        Ok(user_id) => ws.on_upgrade(move |socket| {
            let service = state.gateway_service.clone();
            let shutdown = state.shutdown_rx.clone();
            async move {
                service.handle_socket(socket, user_id, request_id, shutdown).await;
            }
        }),
        Err(e) => {
            tracing::warn!(error = %e, "WebSocket handshake failed: invalid token");
            axum::http::StatusCode::UNAUTHORIZED.into_response()
        }
    }
}
