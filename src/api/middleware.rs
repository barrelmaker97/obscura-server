use crate::api::AppState;
use crate::domain::auth::Jwt;
use crate::error::AppError;
use axum::http::HeaderValue;
use axum::{
    extract::FromRequestParts,
    http::{header, request::Parts},
};
use tower_http::request_id::{MakeRequestId, RequestId};
use uuid::Uuid;

#[derive(Debug)]
pub struct AuthUser {
    pub(crate) user_id: Uuid,
}

impl FromRequestParts<AppState> for AuthUser {
    type Rejection = AppError;

    #[tracing::instrument(err, skip(parts, state))]
    async fn from_request_parts(parts: &mut Parts, state: &AppState) -> Result<Self, Self::Rejection> {
        let auth_header = parts.headers.get(header::AUTHORIZATION).ok_or(AppError::AuthError)?;

        let auth_str = auth_header.to_str().map_err(|_| AppError::AuthError)?;

        if !auth_str.starts_with("Bearer ") {
            return Err(AppError::AuthError);
        }

        let token = &auth_str[7..];
        let jwt = Jwt::new(token.to_string());

        let user_id = state.auth_service.verify_token(&jwt).map_err(|_| AppError::AuthError)?;

        tracing::Span::current().record("user_id", tracing::field::display(user_id));

        Ok(Self { user_id })
    }
}

#[derive(Clone, Debug, Default)]
pub struct MakeRequestUuidOrHeader;

impl MakeRequestId for MakeRequestUuidOrHeader {
    fn make_request_id<B>(&mut self, request: &axum::http::Request<B>) -> Option<RequestId> {
        let header_value = request.headers().get("x-request-id").cloned().unwrap_or_else(|| {
            let uuid = Uuid::new_v4().to_string();
            HeaderValue::from_str(&uuid).expect("Invalid UUID generated for request ID")
        });

        Some(RequestId::new(header_value))
    }
}
