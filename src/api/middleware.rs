use crate::api::AppState;
use crate::error::AppError;
use axum::{
    extract::FromRequestParts,
    http::{header, request::Parts},
};
use jsonwebtoken::{DecodingKey, EncodingKey, Header, Validation, decode, encode};
use serde::{Deserialize, Serialize};
use std::time::{SystemTime, UNIX_EPOCH};
use uuid::Uuid;

#[derive(Debug, Serialize, Deserialize)]
pub struct Claims {
    pub sub: Uuid,
    pub exp: usize,
}

pub struct AuthUser {
    pub user_id: Uuid,
}

impl FromRequestParts<AppState> for AuthUser {
    type Rejection = AppError;

    async fn from_request_parts(
        parts: &mut Parts,
        state: &AppState,
    ) -> Result<Self, Self::Rejection> {
        let auth_header = parts
            .headers
            .get(header::AUTHORIZATION)
            .ok_or(AppError::AuthError)?;

        let auth_str = auth_header.to_str().map_err(|_| AppError::AuthError)?;
        if !auth_str.starts_with("Bearer ") {
            return Err(AppError::AuthError);
        }

        let token = &auth_str[7..];

        let claims = verify_jwt(token, &state.config.jwt_secret)?;

        Ok(AuthUser {
            user_id: claims.sub,
        })
    }
}

pub fn create_jwt(user_id: Uuid, secret: &str) -> Result<String, AppError> {
    let expiration = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap()
        .as_secs() as usize
        + 3600 * 24; // 24 hours

    let claims = Claims {
        sub: user_id,
        exp: expiration,
    };

    encode(
        &Header::default(),
        &claims,
        &EncodingKey::from_secret(secret.as_bytes()),
    )
    .map_err(|_| AppError::Internal)
}

pub fn verify_jwt(token: &str, secret: &str) -> Result<Claims, AppError> {
    let token_data = decode::<Claims>(
        token,
        &DecodingKey::from_secret(secret.as_bytes()),
        &Validation::default(),
    )
    .map_err(|_| AppError::AuthError)?;
    Ok(token_data.claims)
}
