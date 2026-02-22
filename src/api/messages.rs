use crate::api::AppState;
use crate::api::middleware::AuthUser;
use crate::error::{AppError, Result};
use crate::proto::obscura::v1::SendMessageRequest;
use axum::{body::Bytes, extract::State, http::HeaderMap, response::IntoResponse};
use prost::Message;
use uuid::Uuid;

/// Sends a batch of encrypted messages.
///
/// # Errors
/// Returns `AppError::BadRequest` if the request protobuf is malformed.
pub async fn send_messages(
    auth_user: AuthUser,
    State(state): State<AppState>,
    headers: HeaderMap,
    body: Bytes,
) -> Result<impl IntoResponse> {
    let request = SendMessageRequest::decode(body)
        .map_err(|e| AppError::BadRequest(format!("Invalid SendMessageRequest protobuf: {e}")))?;

    if request.messages.len() > usize::try_from(state.config.messaging.send_batch_limit).unwrap_or(0) {
        return Err(AppError::PayloadTooLarge);
    }

    let idempotency_key = headers
        .get("idempotency-key")
        .and_then(|v| v.to_str().ok())
        .ok_or_else(|| AppError::BadRequest("Missing idempotency-key header".to_string()))
        .and_then(|s| Uuid::parse_str(s).map_err(|e| AppError::BadRequest(format!("Invalid idempotency-key: {e}"))))?;

    let response = state.message_service.send_batch(auth_user.user_id, idempotency_key, request.messages).await?;

    Ok(response.encode_to_vec())
}
