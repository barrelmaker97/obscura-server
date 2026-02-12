use crate::api::AppState;
use crate::api::middleware::AuthUser;
use crate::error::{AppError, Result};
use crate::proto::obscura::v1::EncryptedMessage;
use axum::{
    body::Bytes,
    extract::{Path, State},
    http::StatusCode,
    response::IntoResponse,
};
use prost::Message;
use uuid::Uuid;

/// Sends an encrypted message to a recipient.
///
/// # Errors
/// Returns `AppError::BadRequest` if the message protobuf is malformed.
/// Returns `AppError::NotFound` if the recipient does not exist.
pub async fn send_message(
    auth_user: AuthUser,

    State(state): State<AppState>,

    Path(recipient_id): Path<Uuid>,

    body: Bytes,
) -> Result<impl IntoResponse> {
    let msg = EncryptedMessage::decode(body)
        .map_err(|e| AppError::BadRequest(format!("Invalid EncryptedMessage protobuf: {e}")))?;

    state.message_service.send_message(auth_user.user_id, recipient_id, msg.r#type, msg.content).await?;

    Ok(StatusCode::CREATED)
}
