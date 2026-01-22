use crate::api::AppState;
use crate::core::auth::verify_jwt;
use crate::error::AppError;
use axum::{
    extract::FromRequestParts,
    http::{header, request::Parts},
};
use uuid::Uuid;

pub struct AuthUser {
    pub user_id: Uuid,
}

impl FromRequestParts<AppState> for AuthUser {
    type Rejection = AppError;

    async fn from_request_parts(parts: &mut Parts, state: &AppState) -> Result<Self, Self::Rejection> {
        let auth_header = parts.headers.get(header::AUTHORIZATION).ok_or(AppError::AuthError)?;

        let auth_str = auth_header.to_str().map_err(|_| AppError::AuthError)?;
        if !auth_str.starts_with("Bearer ") {
            return Err(AppError::AuthError);
        }

        let token = &auth_str[7..];

        let claims = verify_jwt(token, &state.config.auth.jwt_secret)?;

        Ok(AuthUser { user_id: claims.sub })
    }
}
