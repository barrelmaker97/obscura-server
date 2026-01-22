use crate::api::AppState;
use crate::api::middleware::AuthUser;
use crate::error::Result;
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
    state.message_service.send_message(auth_user.user_id, recipient_id, body).await?;
    Ok(StatusCode::CREATED)
}
