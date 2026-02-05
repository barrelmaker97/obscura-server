use crate::api::AppState;
use crate::core::auth::verify_jwt;
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
        .map(|id| id.header_value().to_str().unwrap_or_default().to_string())
        .unwrap_or_else(|| "unknown".to_string());

    match verify_jwt(&params.token, &state.config.auth.jwt_secret) {
        Ok(claims) => ws.on_upgrade(move |socket| {
            let service = state.gateway_service.clone();
            let shutdown = state.shutdown_rx.clone();
            async move {
                service.handle_socket(socket, claims.sub, request_id, shutdown).await
            }
        }),
        Err(e) => {
            tracing::warn!(error = %e, "WebSocket handshake failed: invalid token");
            axum::http::StatusCode::UNAUTHORIZED.into_response()
        }
    }
}
