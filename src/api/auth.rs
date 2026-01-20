use crate::api::AppState;
use crate::api::middleware::{AuthUser, create_jwt};
use crate::core::auth;
use crate::error::{AppError, Result};
use crate::storage::key_repo::KeyRepository;
use crate::storage::refresh_token_repo::RefreshTokenRepository;
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
pub struct RefreshRequest {
    pub refresh_token: String,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct LogoutRequest {
    pub refresh_token: String,
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
#[serde(rename_all = "camelCase")]
pub struct AuthResponse {
    pub token: String,
    pub refresh_token: String,
    pub expires_at: i64,
}

pub async fn login(State(state): State<AppState>, Json(payload): Json<LoginRequest>) -> Result<impl IntoResponse> {
    let user_repo = UserRepository::new();
    let refresh_repo = RefreshTokenRepository::new(state.pool.clone());

    let user = user_repo.find_by_username(&state.pool, &payload.username).await?.ok_or(AppError::AuthError)?;

    let password = payload.password.clone();
    let password_hash = user.password_hash.clone();

    let is_valid = tokio::task::spawn_blocking(move || auth::verify_password(&password, &password_hash))
        .await
        .map_err(|_| AppError::Internal)??;

    if !is_valid {
        return Err(AppError::AuthError);
    }

    // Generate Tokens
    let token = create_jwt(user.id, &state.config.auth.jwt_secret, state.config.auth.access_token_ttl_secs)?;
    let refresh_token = auth::generate_opaque_token();
    let refresh_hash = auth::hash_token(&refresh_token);

    let mut tx = state.pool.begin().await?;
    refresh_repo.create(&mut tx, user.id, &refresh_hash, state.config.auth.refresh_token_ttl_days).await?;
    tx.commit().await?;

    let expires_at = (time::OffsetDateTime::now_utc()
        + time::Duration::seconds(state.config.auth.access_token_ttl_secs as i64))
    .unix_timestamp();

    Ok(Json(AuthResponse { token, refresh_token, expires_at }))
}

pub async fn register(
    State(state): State<AppState>,
    Json(payload): Json<RegistrationRequest>,
) -> Result<impl IntoResponse> {
    let user_repo = UserRepository::new();
    let key_repo = KeyRepository::new(state.pool.clone());
    let refresh_repo = RefreshTokenRepository::new(state.pool.clone());

    let password = payload.password.clone();
    let password_hash =
        tokio::task::spawn_blocking(move || auth::hash_password(&password)).await.map_err(|_| AppError::Internal)??;

    let mut tx = state.pool.begin().await?;

    let user = user_repo.create(&mut *tx, &payload.username, &password_hash).await.map_err(|e| {
        if let AppError::Database(sqlx::Error::Database(db_err)) = &e
            && db_err.code().as_deref() == Some("23505")
        {
            return AppError::BadRequest("Username already exists".into());
        }
        e
    })?;

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
    key_repo.insert_one_time_pre_keys(&mut tx, user.id, &otpk_vec).await?;

    // Generate Tokens
    let token = create_jwt(user.id, &state.config.auth.jwt_secret, state.config.auth.access_token_ttl_secs)?;
    let refresh_token = auth::generate_opaque_token();
    let refresh_hash = auth::hash_token(&refresh_token);

    refresh_repo.create(&mut tx, user.id, &refresh_hash, state.config.auth.refresh_token_ttl_days).await?;

    tx.commit().await?;

    let expires_at = (time::OffsetDateTime::now_utc()
        + time::Duration::seconds(state.config.auth.access_token_ttl_secs as i64))
    .unix_timestamp();

    Ok((StatusCode::CREATED, Json(AuthResponse { token, refresh_token, expires_at })))
}

pub async fn refresh(State(state): State<AppState>, Json(payload): Json<RefreshRequest>) -> Result<impl IntoResponse> {
    let refresh_repo = RefreshTokenRepository::new(state.pool.clone());

    // 1. Hash the incoming token to look it up
    let hash = auth::hash_token(&payload.refresh_token);

    // 2. Verify and Rotate (Atomic Transaction)
    let mut tx = state.pool.begin().await?;

    let user_id = refresh_repo.verify_and_consume(&mut tx, &hash).await?.ok_or(AppError::AuthError)?;

    // 3. Generate New Pair
    let new_access_token = create_jwt(user_id, &state.config.auth.jwt_secret, state.config.auth.access_token_ttl_secs)?;
    let new_refresh_token = auth::generate_opaque_token();
    let new_refresh_hash = auth::hash_token(&new_refresh_token);

    // 4. Store New Refresh Token
    refresh_repo.create(&mut tx, user_id, &new_refresh_hash, state.config.auth.refresh_token_ttl_days).await?;

    tx.commit().await?;

    let expires_at = (time::OffsetDateTime::now_utc()
        + time::Duration::seconds(state.config.auth.access_token_ttl_secs as i64))
    .unix_timestamp();

    Ok(Json(AuthResponse { token: new_access_token, refresh_token: new_refresh_token, expires_at }))
}

pub async fn logout(
    auth_user: AuthUser,
    State(state): State<AppState>,
    Json(payload): Json<LogoutRequest>,
) -> Result<impl IntoResponse> {
    let refresh_repo = RefreshTokenRepository::new(state.pool.clone());
    let hash = auth::hash_token(&payload.refresh_token);

    refresh_repo.delete_owned(&hash, auth_user.user_id).await?;

    Ok(StatusCode::OK)
}
