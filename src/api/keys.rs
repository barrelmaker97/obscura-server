use crate::api::AppState;
use crate::api::schemas::keys::{PreKeyUpload, PreKeyBundle as PreKeyBundleSchema};
use crate::api::middleware::AuthUser;
use crate::services::key_service::KeyUploadParams;
use crate::error::{AppError, Result};
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
        Some(b) => Ok(Json(PreKeyBundleSchema::from(b))),
        None => Err(crate::error::AppError::NotFound),
    }
}

pub async fn upload_keys(
    auth_user: AuthUser,
    State(state): State<AppState>,
    Json(payload): Json<PreKeyUpload>,
) -> Result<impl IntoResponse> {
    payload.validate().map_err(AppError::BadRequest)?;

    // Call Service directly with domain types from payload
    let params = KeyUploadParams {
        user_id: auth_user.user_id,
        identity_key: payload.identity_key
            .map(|k| k.try_into())
            .transpose()
            .map_err(AppError::BadRequest)?,
        registration_id: payload.registration_id,
        signed_pre_key: payload.signed_pre_key.try_into().map_err(AppError::BadRequest)?,
        one_time_pre_keys: payload.one_time_pre_keys
            .into_iter()
            .map(|k| k.try_into())
            .collect::<std::result::Result<Vec<_>, _>>()
            .map_err(AppError::BadRequest)?,
    };

    state.account_service.upload_keys(params).await?;

    Ok(StatusCode::OK)
}