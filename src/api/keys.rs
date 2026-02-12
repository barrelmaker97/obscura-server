use crate::api::AppState;
use crate::api::middleware::AuthUser;
use crate::api::schemas::keys::{PreKeyBundleResponse, PreKeyUploadRequest};
use crate::error::{AppError, Result};
use crate::services::key_service::KeyUploadParams;
use axum::{
    Json,
    extract::{Path, State},
    http::StatusCode,
    response::IntoResponse,
};
use std::convert::TryInto;
use uuid::Uuid;

/// Fetches a pre-key bundle for a user.
///
/// # Errors
/// Returns `AppError::NotFound` if the user or their keys do not exist.
pub async fn get_pre_key_bundle(State(state): State<AppState>, Path(user_id): Path<Uuid>) -> Result<impl IntoResponse> {
    let bundle = state.key_service.get_pre_key_bundle(user_id).await?;

    bundle.map_or_else(|| Err(AppError::NotFound), |b| Ok(Json(PreKeyBundleResponse::from(b))))
}

/// Uploads new pre-keys for the authenticated user.
///
/// # Errors
/// Returns `AppError::BadRequest` if the keys are malformed or validation fails.
pub async fn upload_keys(
    auth_user: AuthUser,
    State(state): State<AppState>,
    Json(payload): Json<PreKeyUploadRequest>,
) -> Result<impl IntoResponse> {
    payload.validate().map_err(AppError::BadRequest)?;

    // Call Service directly with domain types from payload
    let params = KeyUploadParams {
        user_id: auth_user.user_id,
        identity_key: payload.identity_key.map(TryInto::try_into).transpose().map_err(AppError::BadRequest)?,
        registration_id: payload.registration_id,
        signed_pre_key: payload.signed_pre_key.try_into().map_err(AppError::BadRequest)?,
        one_time_pre_keys: payload
            .one_time_pre_keys
            .into_iter()
            .map(TryInto::try_into)
            .collect::<std::result::Result<Vec<_>, _>>()
            .map_err(AppError::BadRequest)?,
    };

    state.account_service.upload_keys(params).await?;

    Ok(StatusCode::OK)
}
