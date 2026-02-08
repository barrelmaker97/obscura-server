use crate::api::dto::crypto::PublicKeyDto;
use crate::api::dto::keys::{OneTimePreKeyDto, SignedPreKeyDto};
use serde::{Deserialize, Serialize};

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct RegistrationRequest {
    pub username: String,
    pub password: String,
    pub identity_key: PublicKeyDto,
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

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct AuthResponse {
    pub token: String,
    pub refresh_token: String,
    pub expires_at: i64,
}