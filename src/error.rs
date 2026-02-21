use crate::api::schemas::common::ErrorResponse;
use axum::{
    Json,
    http::StatusCode,
    response::{IntoResponse, Response},
};
use thiserror::Error;

#[derive(Error, Debug)]
pub enum AppError {
    #[error("Database error: {0}")]
    Database(#[from] sqlx::Error),
    #[error("Authentication failed")]
    AuthError,
    #[error("Not found")]
    NotFound,
    #[error("Invalid request: {0}")]
    BadRequest(String),
    #[error("Conflict: {0}")]
    Conflict(String),
    #[error("Precondition failed")]
    PreconditionFailed,
    #[error("Request timeout")]
    Timeout,
    #[error("Length required")]
    LengthRequired,
    #[error("Payload too large")]
    PayloadTooLarge,
    #[error("Internal server error")]
    Internal,
    #[error("Internal error: {0}")]
    InternalMsg(String),
}

pub type Result<T> = std::result::Result<T, AppError>;

impl IntoResponse for AppError {
    fn into_response(self) -> Response {
        let (status, message) = match self {
            Self::AuthError => (StatusCode::UNAUTHORIZED, "Unauthorized".to_string()),
            Self::NotFound => (StatusCode::NOT_FOUND, "Not found".to_string()),
            Self::BadRequest(msg) => (StatusCode::BAD_REQUEST, msg),
            Self::Conflict(msg) => (StatusCode::CONFLICT, msg),
            Self::PreconditionFailed => (StatusCode::PRECONDITION_FAILED, "Precondition failed".to_string()),
            Self::Timeout => (StatusCode::REQUEST_TIMEOUT, "Request timeout".to_string()),
            Self::LengthRequired => (StatusCode::LENGTH_REQUIRED, "Length required".to_string()),
            Self::PayloadTooLarge => (StatusCode::PAYLOAD_TOO_LARGE, "Payload too large".to_string()),
            Self::Database(_) | Self::Internal | Self::InternalMsg(_) => {
                (StatusCode::INTERNAL_SERVER_ERROR, "Internal server error".to_string())
            }
        };

        let body = Json(ErrorResponse { error: message });

        (status, body).into_response()
    }
}
