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
        let auth_header = parts.headers.get(header::AUTHORIZATION).ok_or_else(|| {
            tracing::debug!("Missing Authorization header");
            AppError::AuthError
        })?;

        let auth_str = auth_header.to_str().map_err(|_| {
            tracing::debug!("Invalid Authorization header encoding");
            AppError::AuthError
        })?;

        if !auth_str.starts_with("Bearer ") {
            tracing::debug!("Authorization header does not start with 'Bearer '");
            return Err(AppError::AuthError);
        }

        let token = &auth_str[7..];

        let claims = verify_jwt(token, &state.config.auth.jwt_secret).map_err(|e| {
            tracing::debug!("JWT verification failed: {:?}", e);
            e
        })?;

        tracing::Span::current().record("user_id", claims.sub.to_string());

        Ok(AuthUser { user_id: claims.sub })
    }
}
