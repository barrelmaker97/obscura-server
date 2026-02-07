use crate::api::AppState;
use crate::api::dto::keys::PreKeyUpload;
use crate::api::middleware::AuthUser;
use crate::core::key_service::KeyUploadParams;
use crate::error::Result;
use axum::{
    Json,
    extract::{Path, State},
    http::StatusCode,
    response::IntoResponse,
};
use uuid::Uuid;

pub async fn get_pre_key_bundle(State(state): State<AppState>, Path(user_id): Path<Uuid>) -> Result<impl IntoResponse> {
    let bundle = state.key_service.get_pre_key_bundle(user_id).await?;

    match bundle {
        Some(b) => Ok(Json(b)),
        None => Err(crate::error::AppError::NotFound),
    }
}

pub async fn upload_keys(
    auth_user: AuthUser,
    State(state): State<AppState>,
    Json(payload): Json<PreKeyUpload>,
) -> Result<impl IntoResponse> {
    // Call Service directly with domain types from payload
    let params = KeyUploadParams {
        user_id: auth_user.user_id,
        identity_key: payload.identity_key,
        registration_id: payload.registration_id,
        signed_pre_key: payload.signed_pre_key,
        one_time_pre_keys: payload.one_time_pre_keys,
    };

    state.account_service.upload_keys(params).await?;

    Ok(StatusCode::OK)
}