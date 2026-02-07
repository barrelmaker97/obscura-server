use crate::api::AppState;
use crate::api::auth::{OneTimePreKeyDto, SignedPreKeyDto};
use crate::api::middleware::AuthUser;
use crate::core::crypto_types::PublicKey;
use crate::core::key_service::KeyUploadParams;
use crate::core::user::{OneTimePreKey, SignedPreKey};
use crate::error::Result;
use axum::{
    Json,
    extract::{Path, State},
    http::StatusCode,
    response::IntoResponse,
};
use uuid::Uuid;

#[derive(serde::Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PreKeyUpload {
    pub identity_key: Option<PublicKey>,
    pub registration_id: Option<i32>,
    pub signed_pre_key: SignedPreKeyDto,
    pub one_time_pre_keys: Vec<OneTimePreKeyDto>,
}

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
    // 1. Identity Key (already parsed)
    let identity_key = payload.identity_key;

    let signed_pre_key = SignedPreKey {
        key_id: payload.signed_pre_key.key_id,
        public_key: payload.signed_pre_key.public_key,
        signature: payload.signed_pre_key.signature,
    };

    // 2. One-Time Pre-Keys (already parsed)
    let mut one_time_pre_keys = Vec::with_capacity(payload.one_time_pre_keys.len());
    for k in payload.one_time_pre_keys {
        one_time_pre_keys.push(OneTimePreKey { key_id: k.key_id, public_key: k.public_key });
    }

    // 3. Call Service
    let params = KeyUploadParams {
        user_id: auth_user.user_id,
        identity_key,
        registration_id: payload.registration_id,
        signed_pre_key,
        one_time_pre_keys,
    };

    state.account_service.upload_keys(params).await?;

    Ok(StatusCode::OK)
}
