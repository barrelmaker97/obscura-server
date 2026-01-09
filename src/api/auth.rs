use axum::{extract::State, http::StatusCode, Json, response::IntoResponse};
use serde::{Deserialize, Serialize};
use crate::api::AppState;
use crate::core::auth;
use crate::storage::user_repo::UserRepository;
use crate::storage::key_repo::KeyRepository;
use crate::error::{Result, AppError};
use base64::{Engine as _, engine::general_purpose::STANDARD};
use crate::api::middleware::create_jwt;

#[derive(Deserialize)]
pub struct RegistrationRequest {
    pub username: String,
    pub password: String,
    #[serde(rename = "identityKey")]
    pub identity_key: String, // Base64
    #[serde(rename = "registrationId")]
    pub registration_id: i32,
    #[serde(rename = "signedPreKey")]
    pub signed_pre_key: SignedPreKeyDto,
    #[serde(rename = "oneTimePreKeys")]
    pub one_time_pre_keys: Vec<OneTimePreKeyDto>,
}

#[derive(Deserialize)]
pub struct SignedPreKeyDto {
    #[serde(rename = "keyId")]
    pub key_id: i32,
    #[serde(rename = "publicKey")]
    pub public_key: String,
    pub signature: String,
}

#[derive(Deserialize)]
pub struct OneTimePreKeyDto {
    #[serde(rename = "keyId")]
    pub key_id: i32,
    #[serde(rename = "publicKey")]
    pub public_key: String,
}

#[derive(Serialize)]
pub struct AuthResponse {
    pub token: String,
}

pub async fn register(
    State(state): State<AppState>,
    Json(payload): Json<RegistrationRequest>,
) -> Result<impl IntoResponse> {
    let user_repo = UserRepository::new(state.pool.clone());
    let key_repo = KeyRepository::new(state.pool.clone());

    // 1. Create User
    let password_hash = auth::hash_password(&payload.password)?;
    let user = user_repo.create(&payload.username, &password_hash).await.map_err(|e| {
        if let AppError::Database(sqlx::Error::Database(db_err)) = &e {
            if db_err.code().as_deref() == Some("23505") {
                 return AppError::BadRequest("Username already exists".into());
            }
        }
        e
    })?;

    // 2. Upload Keys
    let identity_key_bytes = STANDARD.decode(&payload.identity_key).map_err(|_| AppError::BadRequest("Invalid base64 identityKey".into()))?;

    key_repo.upsert_identity_key(user.id, &identity_key_bytes, payload.registration_id).await?;

    let spk_pub = STANDARD.decode(&payload.signed_pre_key.public_key).map_err(|_| AppError::BadRequest("Invalid base64 signedPreKey public key".into()))?;
    let spk_sig = STANDARD.decode(&payload.signed_pre_key.signature).map_err(|_| AppError::BadRequest("Invalid base64 signedPreKey signature".into()))?;
    key_repo.upsert_signed_pre_key(user.id, payload.signed_pre_key.key_id, &spk_pub, &spk_sig).await?;

    let mut otpk_vec = Vec::new();
    for k in payload.one_time_pre_keys {
        let pub_key = STANDARD.decode(&k.public_key).map_err(|_| AppError::BadRequest("Invalid base64 oneTimePreKey".into()))?;
        otpk_vec.push((k.key_id, pub_key));
    }
    key_repo.insert_one_time_pre_keys(user.id, &otpk_vec).await?;

    // 3. Generate Token
    let token = create_jwt(user.id, &state.config.jwt_secret)?;

    Ok((StatusCode::CREATED, Json(AuthResponse { token })))
}