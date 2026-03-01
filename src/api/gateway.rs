use crate::api::AppState;
use crate::api::schemas::gateway::{TicketResponse, WsParams};
use axum::{
    extract::{Query, State, ws::WebSocketUpgrade},
    http::Extensions,
    response::IntoResponse,
};
use tower_http::request_id::RequestId;

pub(crate) async fn generate_ticket(
    auth_user: crate::api::middleware::AuthUser,
    State(state): State<AppState>,
) -> Result<impl IntoResponse, crate::error::AppError> {
    let ticket = uuid::Uuid::new_v4().to_string();
    state.ws_ticket_cache.set(&ticket, auth_user.user_id.to_string().as_bytes()).await.map_err(|e| {
        tracing::error!(error = %e, "Failed to cache websocket ticket");
        crate::error::AppError::InternalMsg("Failed to generate ticket".to_string())
    })?;

    Ok((axum::http::StatusCode::CREATED, axum::Json(TicketResponse { ticket })))
}

pub(crate) async fn websocket_handler(
    ws: WebSocketUpgrade,
    Query(params): Query<WsParams>,
    extensions: Extensions,
    State(state): State<AppState>,
) -> impl IntoResponse {
    let request_id = extensions
        .get::<RequestId>()
        .map_or_else(|| "unknown".to_string(), |id| id.header_value().to_str().unwrap_or_default().to_string());

    // Validate ticket
    let user_id_res = match state.ws_ticket_cache.get(&params.ticket).await {
        Ok(Some(bytes)) => match String::from_utf8(bytes) {
            Ok(id_str) => match uuid::Uuid::parse_str(&id_str) {
                Ok(id) => {
                    // Delete ticket so it can only be used once
                    let _ = state.ws_ticket_cache.delete(&params.ticket).await;
                    Ok(id)
                }
                Err(_) => Err("Invalid user ID format in cache".to_string()),
            },
            Err(_) => Err("Invalid UTF-8 in cache".to_string()),
        },
        Ok(None) => Err("Ticket not found or expired".to_string()),
        Err(e) => {
            tracing::error!(error = %e, "Failed to read ticket from cache");
            Err("Internal server error".to_string())
        }
    };

    match user_id_res {
        Ok(user_id) => ws.on_upgrade(move |socket| {
            let service = state.gateway_service.clone();
            let shutdown = state.shutdown_rx.clone();
            async move {
                service.handle_socket(socket, user_id, request_id, shutdown).await;
            }
        }),
        Err(e) => {
            tracing::warn!(error = %e, "WebSocket handshake failed: invalid ticket");
            axum::http::StatusCode::UNAUTHORIZED.into_response()
        }
    }
}
