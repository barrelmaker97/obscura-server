use crate::api::AppState;
use crate::api::middleware::AuthUser;
use crate::api::schemas::push_tokens::RegisterPushTokenRequest;
use crate::error::{AppError, Result};
use axum::{Json, extract::State, http::StatusCode, response::IntoResponse};

/// Registers or updates a push token for the authenticated device.
///
/// # Errors
/// Returns `AppError::AuthError` if the user is not authenticated.
/// Returns `AppError::BadRequest` if the token format is invalid or no device_id in token.
/// Returns `AppError::Database` if the database operation fails.
pub(crate) async fn register_token(
    auth_user: AuthUser,
    State(state): State<AppState>,
    Json(payload): Json<RegisterPushTokenRequest>,
) -> Result<impl IntoResponse> {
    let device_id = auth_user.device_id.ok_or(AppError::BadRequest("Device-scoped token required".to_string()))?;

    payload.validate().map_err(AppError::BadRequest)?;

    state.push_token_service.register_token(device_id, payload.token).await?;
    Ok(StatusCode::OK)
}
