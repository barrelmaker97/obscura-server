use crate::api::AppState;
use crate::api::schemas::auth::{
    AuthSession as AuthSessionSchema, Login, Logout, Refresh, Registration,
};
use crate::api::middleware::AuthUser;
use crate::domain::auth_session::AuthSession;
use crate::error::{AppError, Result};
use axum::{Json, extract::State, http::StatusCode, response::IntoResponse};

pub async fn login(State(state): State<AppState>, Json(payload): Json<Login>) -> Result<impl IntoResponse> {
    let session = state.account_service.login(payload.username, payload.password).await?;
    let auth_response = map_session(session);
    Ok(Json(auth_response))
}

pub async fn register(
    State(state): State<AppState>,
    Json(payload): Json<Registration>,
) -> Result<impl IntoResponse> {
    let session = state
        .account_service
        .register(
            payload.username,
            payload.password,
            payload.identity_key.try_into().map_err(AppError::BadRequest)?,
            payload.registration_id,
            payload.signed_pre_key.try_into().map_err(AppError::BadRequest)?,
            payload.one_time_pre_keys
                .into_iter()
                .map(|k| k.try_into())
                .collect::<std::result::Result<Vec<_>, _>>()
                .map_err(AppError::BadRequest)?,
        )
        .await?;

    let auth_response = map_session(session);
    Ok((StatusCode::CREATED, Json(auth_response)))
}

pub async fn refresh(State(state): State<AppState>, Json(payload): Json<Refresh>) -> Result<impl IntoResponse> {
    let session = state.account_service.refresh(payload.refresh_token).await?;
    let auth_response = map_session(session);
    Ok(Json(auth_response))
}

pub async fn logout(
    auth_user: AuthUser,
    State(state): State<AppState>,
    Json(payload): Json<Logout>,
) -> Result<impl IntoResponse> {
    state.account_service.logout(auth_user.user_id, payload.refresh_token).await?;
    Ok(StatusCode::OK)
}

fn map_session(session: AuthSession) -> AuthSessionSchema {
    AuthSessionSchema {
        token: session.token,
        refresh_token: session.refresh_token,
        expires_at: session.expires_at,
    }
}