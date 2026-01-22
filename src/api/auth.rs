use crate::api::AppState;
use crate::api::middleware::AuthUser;
use crate::core::crypto_types::{PublicKey, Signature};
use crate::core::user::{OneTimePreKey, SignedPreKey};
use crate::error::Result;
use axum::{Json, extract::State, http::StatusCode, response::IntoResponse};
use serde::Deserialize;

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct RegistrationRequest {
    pub username: String,
    pub password: String,
    pub identity_key: PublicKey,
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
    pub public_key: PublicKey,
    pub signature: Signature,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct OneTimePreKeyDto {
    pub key_id: i32,
    pub public_key: PublicKey,
}

pub async fn login(State(state): State<AppState>, Json(payload): Json<LoginRequest>) -> Result<impl IntoResponse> {
    let auth_response = state.account_service.login(payload.username, payload.password).await?;
    Ok(Json(auth_response))
}

pub async fn register(
    State(state): State<AppState>,
    Json(payload): Json<RegistrationRequest>,
) -> Result<impl IntoResponse> {
    let mut otpk_vec = Vec::new();
    for k in payload.one_time_pre_keys {
        otpk_vec.push(OneTimePreKey { key_id: k.key_id, public_key: k.public_key });
    }

    let auth_response = state
        .account_service
        .register(
            payload.username,
            payload.password,
            payload.identity_key,
            payload.registration_id,
            SignedPreKey {
                key_id: payload.signed_pre_key.key_id,
                public_key: payload.signed_pre_key.public_key,
                signature: payload.signed_pre_key.signature,
            },
            otpk_vec,
        )
        .await?;

    Ok((StatusCode::CREATED, Json(auth_response)))
}

pub async fn refresh(State(state): State<AppState>, Json(payload): Json<RefreshRequest>) -> Result<impl IntoResponse> {
    let auth_response = state.account_service.refresh(payload.refresh_token).await?;
    Ok(Json(auth_response))
}

pub async fn logout(
    auth_user: AuthUser,
    State(state): State<AppState>,
    Json(payload): Json<LogoutRequest>,
) -> Result<impl IntoResponse> {
    state.account_service.logout(auth_user.user_id, payload.refresh_token).await?;
    Ok(StatusCode::OK)
}
