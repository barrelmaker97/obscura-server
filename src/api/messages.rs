use axum::{
    extract::{Path, State},
    http::StatusCode,
    response::IntoResponse,
    body::Bytes,
};
use prost::Message as ProstMessage; 
use uuid::Uuid;
use crate::api::AppState;
use crate::api::middleware::AuthUser;
use crate::storage::message_repo::MessageRepository;
use crate::core::message_service::MessageService;
use crate::proto::obscura::v1::OutgoingMessage;
use crate::error::{Result, AppError};

pub async fn send_message(
    auth_user: AuthUser,
    State(state): State<AppState>,
    Path(destination_device_id): Path<Uuid>,
    body: Bytes,
) -> Result<impl IntoResponse> {
    // Validate Protobuf
    let _ = OutgoingMessage::decode(body.clone())
        .map_err(|_| AppError::BadRequest("Invalid protobuf".into()))?;
    
    let service = MessageService::new(MessageRepository::new(state.pool));
    
    // Store raw body (OutgoingMessage serialized)
    service.enqueue_message(auth_user.user_id, destination_device_id, body.to_vec()).await?;

    Ok(StatusCode::CREATED)
}
