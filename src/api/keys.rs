use crate::api::AppState;
use crate::api::auth::{OneTimePreKeyDto, SignedPreKeyDto};
use crate::api::middleware::AuthUser;
use crate::core::notification::UserEvent;
use crate::error::{AppError, Result};
use crate::storage::key_repo::KeyRepository;
use crate::storage::message_repo::MessageRepository;
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
    let key_repo = KeyRepository::new(state.pool);
    let bundle = key_repo.fetch_pre_key_bundle(user_id).await?;

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
    let key_repo = KeyRepository::new(state.pool.clone());

    // Start transaction immediately for atomic check-and-set
    let mut tx = state.pool.begin().await?;

    let mut is_takeover = false;
    let mut new_ik_bytes = Vec::new();

    // Check Identity Key if provided
    if let Some(new_ik_b64) = &payload.identity_key {
        new_ik_bytes =
            STANDARD.decode(new_ik_b64).map_err(|_| AppError::BadRequest("Invalid base64 identityKey".into()))?;

        // Fetch existing identity key with LOCK
        let existing_ik_opt = key_repo.fetch_identity_key_for_update(&mut *tx, auth_user.user_id).await?;

        if let Some(existing_ik) = existing_ik_opt {
            if existing_ik != new_ik_bytes {
                is_takeover = true;
            }
        } else {
            // No existing key? Treat as takeover to ensure clean slate.
            is_takeover = true;
        }
    }

    if is_takeover {
        let reg_id =
            payload.registration_id.ok_or(AppError::BadRequest("registrationId required for takeover".into()))?;

        // 1. Delete old pre-keys
        key_repo.delete_all_signed_pre_keys(&mut *tx, auth_user.user_id).await?;
        key_repo.delete_all_one_time_pre_keys(&mut *tx, auth_user.user_id).await?;

        // 2. Delete pending messages
        let message_repo = MessageRepository::new(state.pool.clone());
        message_repo.delete_all_for_user(&mut *tx, auth_user.user_id).await?;

        // 3. Update Identity Key
        key_repo.upsert_identity_key(&mut *tx, auth_user.user_id, &new_ik_bytes, reg_id).await?;
    }

    // Common flow: Upsert Keys (Works for both Refill and Takeover)
    let spk_pub = STANDARD
        .decode(&payload.signed_pre_key.public_key)
        .map_err(|_| AppError::BadRequest("Invalid base64 signedPreKey public key".into()))?;
    let spk_sig = STANDARD
        .decode(&payload.signed_pre_key.signature)
        .map_err(|_| AppError::BadRequest("Invalid base64 signedPreKey signature".into()))?;

    key_repo
        .upsert_signed_pre_key(&mut *tx, auth_user.user_id, payload.signed_pre_key.key_id, &spk_pub, &spk_sig)
        .await?;

    let mut otpk_vec = Vec::new();
    for k in payload.one_time_pre_keys {
        let pub_key =
            STANDARD.decode(&k.public_key).map_err(|_| AppError::BadRequest("Invalid base64 oneTimePreKey".into()))?;
        otpk_vec.push((k.key_id, pub_key));
    }
    key_repo.insert_one_time_pre_keys(&mut tx, auth_user.user_id, &otpk_vec).await?;

    tx.commit().await?;

    if is_takeover {
        // Trigger disconnect
        state.notifier.notify(auth_user.user_id, UserEvent::Disconnect);
    }

    Ok(StatusCode::OK)
}
