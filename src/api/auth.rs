use crate::api::AppState;
use crate::api::middleware::create_jwt;
use crate::core::auth;
use crate::error::{AppError, Result};
use crate::storage::key_repo::KeyRepository;
use crate::storage::user_repo::UserRepository;
use axum::{Json, extract::State, http::StatusCode, response::IntoResponse};
use base64::{Engine as _, engine::general_purpose::STANDARD};
use serde::{Deserialize, Serialize};

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct RegistrationRequest {
    pub username: String,
    pub password: String,
    pub identity_key: String, // Base64
    pub registration_id: i32,
    pub signed_pre_key: SignedPreKeyDto,
    pub one_time_pre_keys: Vec<OneTimePreKeyDto>,
}

#[derive(Deserialize)]
pub struct LoginRequest {
    pub username: String,
    pub password: String,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SignedPreKeyDto {
    pub key_id: i32,
    pub public_key: String,
    pub signature: String,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct OneTimePreKeyDto {
    pub key_id: i32,
    pub public_key: String,
}

#[derive(Serialize)]
pub struct AuthResponse {
    pub token: String,
}

pub async fn login(State(state): State<AppState>, Json(payload): Json<LoginRequest>) -> Result<impl IntoResponse> {
    let user_repo = UserRepository::new();

    let user = user_repo.find_by_username(&state.pool, &payload.username).await?.ok_or(AppError::AuthError)?; // Generic AuthError to avoid enumeration

    if !auth::verify_password(&payload.password, &user.password_hash)? {
        return Err(AppError::AuthError);
    }

    let token = create_jwt(user.id, &state.config.jwt_secret)?;

    Ok(Json(AuthResponse { token }))
}

pub async fn register(
    State(state): State<AppState>,
    Json(payload): Json<RegistrationRequest>,
) -> Result<impl IntoResponse> {
    let user_repo = UserRepository::new();
    let key_repo = KeyRepository::new(state.pool.clone());

    // 1. Hash Password (Blocking, TODO: Offload)
    let password_hash = auth::hash_password(&payload.password)?;

    // 2. Start Transaction
    let mut tx = state.pool.begin().await?;

    // 3. Create User
    let user = user_repo.create(&mut *tx, &payload.username, &password_hash).await.map_err(|e| {
        if let AppError::Database(sqlx::Error::Database(db_err)) = &e
            && db_err.code().as_deref() == Some("23505")
        {
            return AppError::BadRequest("Username already exists".into());
        }
        e
    })?;

    // 4. Upload Keys
    let identity_key_bytes = STANDARD
        .decode(&payload.identity_key)
        .map_err(|_| AppError::BadRequest("Invalid base64 identityKey".into()))?;

    key_repo.upsert_identity_key(&mut *tx, user.id, &identity_key_bytes, payload.registration_id).await?;

    let spk_pub = STANDARD
        .decode(&payload.signed_pre_key.public_key)
        .map_err(|_| AppError::BadRequest("Invalid base64 signedPreKey public key".into()))?;
    let spk_sig = STANDARD
        .decode(&payload.signed_pre_key.signature)
        .map_err(|_| AppError::BadRequest("Invalid base64 signedPreKey signature".into()))?;
    key_repo.upsert_signed_pre_key(&mut *tx, user.id, payload.signed_pre_key.key_id, &spk_pub, &spk_sig).await?;

    let mut otpk_vec = Vec::new();
    for k in payload.one_time_pre_keys {
        let pub_key =
            STANDARD.decode(&k.public_key).map_err(|_| AppError::BadRequest("Invalid base64 oneTimePreKey".into()))?;
        otpk_vec.push((k.key_id, pub_key));
    }
    // Note: insert_one_time_pre_keys takes &mut PgConnection
    key_repo.insert_one_time_pre_keys(&mut tx, user.id, &otpk_vec).await?;

    // 5. Commit Transaction
    tx.commit().await?;

    // 6. Generate Token
    let token = create_jwt(user.id, &state.config.jwt_secret)?;

    Ok((StatusCode::CREATED, Json(AuthResponse { token })))
}
