use crate::api::AppState;
use crate::api::middleware::AuthUser;
use crate::domain::message::RawSubmission;
use crate::error::{AppError, Result};
use crate::proto::obscura::v1 as proto;
use axum::{body::Bytes, extract::State, http::HeaderMap, response::IntoResponse};
use prost::Message;
use uuid::Uuid;

/// Sends a batch of encrypted messages.
///
/// # Errors
/// Returns `AppError::BadRequest` if the request protobuf is malformed.
pub(crate) async fn send_messages(
    auth_user: AuthUser,
    State(state): State<AppState>,
    headers: HeaderMap,
    body: Bytes,
) -> Result<impl IntoResponse> {
    let idempotency_key = headers
        .get("idempotency-key")
        .and_then(|v| v.to_str().ok())
        .ok_or_else(|| AppError::BadRequest("Missing idempotency-key header".to_string()))
        .and_then(|s| Uuid::parse_str(s).map_err(|e| AppError::BadRequest(format!("Invalid idempotency-key: {e}"))))?;

    // 1. Check Idempotency Cache
    if let Ok(Some(cached)) = state.submission_cache.get_response(&idempotency_key.to_string()).await {
        tracing::info!(key = %idempotency_key, "Returning cached idempotency response");
        return Ok(cached);
    }

    // 2. Protocol Validation & Decoding
    let request = proto::SendMessageRequest::decode(body)
        .map_err(|e| AppError::BadRequest(format!("Invalid SendMessageRequest protobuf: {e}")))?;

    if request.messages.len() > usize::try_from(state.config.messaging.send_batch_limit).unwrap_or(0) {
        return Err(AppError::PayloadTooLarge);
    }

    // 3. Simple Domain Mapping (moves only)
    let submissions: Vec<RawSubmission> = request.messages.into_iter().map(RawSubmission::from).collect();

    // 4. Domain Logic: Call Pure Service
    let outcome = state.message_service.send(auth_user.user_id, submissions).await?;

    // 5. Result Mapping
    let response = proto::SendMessageResponse::from(outcome);
    let response_bytes = response.encode_to_vec();

    // 6. Infrastructure: Update Idempotency Cache
    if let Err(e) = state
        .submission_cache
        .save_response(&idempotency_key.to_string(), &response_bytes, state.config.messaging.idempotency_ttl_secs)
        .await
    {
        tracing::error!(error = %e, "Failed to cache idempotency response");
    }

    Ok(response_bytes)
}
