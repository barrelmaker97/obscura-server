use crate::api::AppState;
use crate::api::middleware::AuthUser;
use crate::api::schemas::auth::{AuthResponse, LoginRequest, LogoutRequest, RefreshRequest, RegistrationRequest};
use crate::domain::auth_session::AuthSession;
use crate::error::{AppError, Result};
use axum::{Json, extract::State, http::StatusCode, response::IntoResponse};
use std::convert::TryInto;

/// Authenticates a user and returns a session.
///
/// # Errors
/// Returns `AppError::AuthError` if the credentials are invalid.
pub(crate) async fn login(
    State(state): State<AppState>,
    Json(payload): Json<LoginRequest>,
) -> Result<impl IntoResponse> {
    let session = state.auth_service.login(payload.username.to_lowercase(), payload.password).await?;
    let auth_response = map_session(session);
    Ok(Json(auth_response))
}

/// Registers a new user.
///
/// # Errors
/// Returns `AppError::BadRequest` if validation fails or keys are malformed.
/// Returns `AppError::Conflict` if the username is already taken.
pub(crate) async fn register(
    State(state): State<AppState>,
    Json(payload): Json<RegistrationRequest>,
) -> Result<impl IntoResponse> {
    payload.validate().map_err(AppError::BadRequest)?;

    let session = state
        .account_service
        .register(
            payload.username.to_lowercase(),
            payload.password,
            payload.identity_key.try_into().map_err(AppError::BadRequest)?,
            payload.registration_id,
            payload.signed_pre_key.try_into().map_err(AppError::BadRequest)?,
            payload
                .one_time_pre_keys
                .into_iter()
                .map(TryInto::try_into)
                .collect::<std::result::Result<Vec<_>, _>>()
                .map_err(AppError::BadRequest)?,
        )
        .await?;

    let auth_response = map_session(session);
    Ok((StatusCode::CREATED, Json(auth_response)))
}

/// Rotates a session using a refresh token.
///
/// # Errors
/// Returns `AppError::AuthError` if the refresh token is invalid or expired.
pub(crate) async fn refresh(
    State(state): State<AppState>,
    Json(payload): Json<RefreshRequest>,
) -> Result<impl IntoResponse> {
    let session = state.auth_service.refresh_session(payload.refresh_token).await?;
    let auth_response = map_session(session);
    Ok(Json(auth_response))
}

/// Invalidates a refresh token.
///
/// # Errors
/// Returns `AppError::AuthError` if the user is not authorized.
pub(crate) async fn logout(
    auth_user: AuthUser,
    State(state): State<AppState>,
    Json(payload): Json<LogoutRequest>,
) -> Result<impl IntoResponse> {
    state.auth_service.logout(auth_user.user_id, payload.refresh_token).await?;
    Ok(StatusCode::OK)
}

fn map_session(session: AuthSession) -> AuthResponse {
    AuthResponse { token: session.token, refresh_token: session.refresh_token, expires_at: session.expires_at }
}
