use crate::api::AppState;
use crate::api::middleware::AuthUser;
use crate::core::message_service::MessageService;
use crate::core::notification::UserEvent;
use crate::error::{AppError, Result};
use crate::proto::obscura::v1::OutgoingMessage;
use crate::storage::message_repo::MessageRepository;
use axum::{
    body::Bytes,
    extract::{Path, State},
    http::StatusCode,
    response::IntoResponse,
};
use prost::Message as ProstMessage;
use uuid::Uuid;

pub async fn send_message(
    auth_user: AuthUser,
    State(state): State<AppState>,
    Path(recipient_id): Path<Uuid>,
    body: Bytes,
) -> Result<impl IntoResponse> {
    // Validate Protobuf
    let _ = OutgoingMessage::decode(body.clone()).map_err(|_| AppError::BadRequest("Invalid protobuf".into()))?;

    let service = MessageService::new(MessageRepository::new(state.pool), state.config.clone());

    // Store raw body (OutgoingMessage serialized)
    service.enqueue_message(auth_user.user_id, recipient_id, body.to_vec()).await?;

    // Notify the user if they are connected
    state.notifier.notify(recipient_id, UserEvent::MessageReceived);

    Ok(StatusCode::CREATED)
}
