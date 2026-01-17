use crate::api::AppState;
use crate::api::middleware::AuthUser;
use crate::core::message_service::MessageService;
use crate::core::notification::UserEvent;
use crate::error::Result;
use crate::storage::message_repo::MessageRepository;
use axum::{
    body::Bytes,
    extract::{Path, State},
    http::StatusCode,
    response::IntoResponse,
};
use uuid::Uuid;

pub async fn send_message(
    auth_user: AuthUser,
    State(state): State<AppState>,
    Path(recipient_id): Path<Uuid>,
    body: Bytes,
) -> Result<impl IntoResponse> {
    let service = MessageService::new(MessageRepository::new(state.pool), state.config.clone());

    // Store raw body directly (blind relay)
    service.enqueue_message(auth_user.user_id, recipient_id, body.to_vec()).await?;

    // Notify the user if they are connected
    state.notifier.notify(recipient_id, UserEvent::MessageReceived);

    Ok(StatusCode::CREATED)
}