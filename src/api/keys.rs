use crate::api::AppState;
use crate::api::auth::{OneTimePreKeyDto, SignedPreKeyDto};
use crate::api::middleware::AuthUser;
use crate::core::key_service::KeyUploadParams;
use crate::core::user::{OneTimePreKey, SignedPreKey};
use crate::error::{AppError, Result};
use axum::{
    Json,
    extract::{Path, State},
    http::StatusCode,
    response::IntoResponse,
};
use base64::{Engine as _, engine::general_purpose::STANDARD};
use serde::Deserialize;
use uuid::Uuid;

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PreKeyUpload {
    pub identity_key: Option<String>,
    pub registration_id: Option<i32>,
    pub signed_pre_key: SignedPreKeyDto,
    pub one_time_pre_keys: Vec<OneTimePreKeyDto>,
}

pub async fn get_pre_key_bundle(State(state): State<AppState>, Path(user_id): Path<Uuid>) -> Result<impl IntoResponse> {
    let bundle = state.key_service.get_pre_key_bundle(user_id).await?;

    match bundle {
        Some(b) => {
            if b.one_time_pre_key.is_none() {
                return Err(AppError::BadRequest("No one-time prekeys available".into()));
            }
            Ok(Json(b))
        }
        None => Err(AppError::NotFound),
    }
}

pub async fn upload_keys(
    auth_user: AuthUser,
    State(state): State<AppState>,
    Json(payload): Json<PreKeyUpload>,
) -> Result<impl IntoResponse> {
    // 1. Decode Identity Key if provided
    let identity_key = match payload.identity_key {
        Some(ik_b64) => {
            Some(STANDARD.decode(ik_b64).map_err(|_| AppError::BadRequest("Invalid base64 identityKey".into()))?)
        }
        None => None,
    };

    // 2. Decode Signed Pre-Key
    let spk_pub = STANDARD
        .decode(&payload.signed_pre_key.public_key)
        .map_err(|_| AppError::BadRequest("Invalid base64 signedPreKey public key".into()))?;
    let spk_sig = STANDARD
        .decode(&payload.signed_pre_key.signature)
        .map_err(|_| AppError::BadRequest("Invalid base64 signedPreKey signature".into()))?;

    let signed_pre_key = SignedPreKey { key_id: payload.signed_pre_key.key_id, public_key: spk_pub, signature: spk_sig };

    // 3. Decode One-Time Pre-Keys
    let mut one_time_pre_keys = Vec::with_capacity(payload.one_time_pre_keys.len());
    for k in payload.one_time_pre_keys {
        let pub_key =
            STANDARD.decode(&k.public_key).map_err(|_| AppError::BadRequest("Invalid base64 oneTimePreKey".into()))?;
        one_time_pre_keys.push(OneTimePreKey { key_id: k.key_id, public_key: pub_key });
    }

    // 4. Call Service
    let params = KeyUploadParams {
        user_id: auth_user.user_id,
        identity_key,
        registration_id: payload.registration_id,
        signed_pre_key,
        one_time_pre_keys,
    };

    state.key_service.upload_keys(params).await?;

    Ok(StatusCode::OK)
}