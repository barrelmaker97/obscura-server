use crate::api::AppState;
use crate::api::middleware::AuthUser;
use crate::api::schemas::notifications::RegisterTokenRequest;
use crate::error::Result;
use axum::{Json, extract::State, http::StatusCode, response::IntoResponse};

/// Registers or updates a push token for the authenticated user.
///
/// # Errors
/// Returns `AppError::AuthError` if the user is not authenticated.
/// Returns `AppError::Database` if the database operation fails.
pub async fn register_token(
    auth_user: AuthUser,
    State(state): State<AppState>,
    Json(payload): Json<RegisterTokenRequest>,
) -> Result<impl IntoResponse> {
    state.push_token_service.register_token(auth_user.user_id, payload.token).await?;
    Ok(StatusCode::OK)
}
