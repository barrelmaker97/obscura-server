use axum::{extract::{Path, State}, Json, response::IntoResponse, http::StatusCode};
use uuid::Uuid;
use crate::api::AppState;
use crate::storage::key_repo::KeyRepository;
use crate::error::{Result, AppError};
use crate::api::middleware::AuthUser;
use crate::api::auth::{SignedPreKeyDto, OneTimePreKeyDto};
use serde::Deserialize;
use base64::{Engine as _, engine::general_purpose::STANDARD};

#[derive(Deserialize)]
pub struct PreKeyUpload {
    #[serde(rename = "signedPreKey")]
    pub signed_pre_key: SignedPreKeyDto,
    #[serde(rename = "oneTimePreKeys")]
    pub one_time_pre_keys: Vec<OneTimePreKeyDto>,
}

pub async fn get_pre_key_bundle(
    State(state): State<AppState>,
    Path(user_id): Path<Uuid>,
) -> Result<impl IntoResponse> {
    let key_repo = KeyRepository::new(state.pool);
    let bundle = key_repo.fetch_pre_key_bundle(user_id).await?;

    match bundle {
        Some(b) => {
            if b.one_time_pre_key.is_none() {
                return Err(AppError::BadRequest("No one-time prekeys available".into()));
            }
            Ok(Json(b))
        },
        None => Err(AppError::NotFound),
    }
}

pub async fn upload_keys(
    auth_user: AuthUser,
    State(state): State<AppState>,
    Json(payload): Json<PreKeyUpload>,
) -> Result<impl IntoResponse> {
    let key_repo = KeyRepository::new(state.pool.clone());

    let spk_pub = STANDARD.decode(&payload.signed_pre_key.public_key).map_err(|_| AppError::BadRequest("Invalid base64 signedPreKey public key".into()))?;
    let spk_sig = STANDARD.decode(&payload.signed_pre_key.signature).map_err(|_| AppError::BadRequest("Invalid base64 signedPreKey signature".into()))?;

    // Start transaction for atomic update
    let mut tx = state.pool.begin().await?;

    key_repo.upsert_signed_pre_key(&mut *tx, auth_user.user_id, payload.signed_pre_key.key_id, &spk_pub, &spk_sig).await?;

    let mut otpk_vec = Vec::new();
    for k in payload.one_time_pre_keys {
        let pub_key = STANDARD.decode(&k.public_key).map_err(|_| AppError::BadRequest("Invalid base64 oneTimePreKey".into()))?;
        otpk_vec.push((k.key_id, pub_key));
    }
    // Pass generic executor (tx implements Deref<Target=PgConnection>)
    key_repo.insert_one_time_pre_keys(&mut tx, auth_user.user_id, &otpk_vec).await?;

    tx.commit().await?;

    Ok(StatusCode::OK)
}