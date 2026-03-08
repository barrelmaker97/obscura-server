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

/// Fetches a pre-key bundle for a device.
///
/// # Errors
/// Returns `AppError::NotFound` if the device or their keys do not exist.
pub(crate) async fn get_pre_key_bundle(
    _auth_user: AuthUser,
    State(state): State<AppState>,
    Path(device_id): Path<Uuid>,
) -> Result<impl IntoResponse> {
    let bundle = state.key_service.get_pre_key_bundle(device_id).await?;

    bundle.map_or_else(|| Err(AppError::NotFound), |b| Ok(Json(PreKeyBundleResponse::from(b))))
}

/// Uploads new pre-keys for the authenticated device.
///
/// # Errors
/// Returns `AppError::BadRequest` if the keys are malformed or validation fails.
/// Returns `AppError::AuthError` if no device_id in token.
pub(crate) async fn upload_keys(
    auth_user: AuthUser,
    State(state): State<AppState>,
    Json(payload): Json<PreKeyUploadRequest>,
) -> Result<impl IntoResponse> {
    let device_id = auth_user.device_id.ok_or(AppError::BadRequest("Device-scoped token required".to_string()))?;

    payload.validate().map_err(AppError::BadRequest)?;

    let params = KeyUploadParams {
        device_id,
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

    state.device_service.upload_keys(params).await?;

    Ok(StatusCode::OK)
}
