use axum::{
    Json,
    http::StatusCode,
    response::{IntoResponse, Response},
};
use serde_json::json;
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
    #[error("Internal server error")]
    Internal,
}

pub type Result<T> = std::result::Result<T, AppError>;

impl IntoResponse for AppError {
    fn into_response(self) -> Response {
        let (status, message) = match self {
            AppError::Database(e) => {
                tracing::error!(error = %e, "Database error");
                (StatusCode::INTERNAL_SERVER_ERROR, "Internal server error".to_string())
            }
            AppError::AuthError => {
                tracing::debug!("Authentication failed");
                (StatusCode::UNAUTHORIZED, "Unauthorized".to_string())
            }
            AppError::NotFound => {
                tracing::debug!("Resource not found");
                (StatusCode::NOT_FOUND, "Not found".to_string())
            }
            AppError::BadRequest(msg) => {
                tracing::debug!(message = %msg, "Bad request");
                (StatusCode::BAD_REQUEST, msg)
            }
            AppError::Conflict(msg) => {
                tracing::debug!(message = %msg, "Conflict");
                (StatusCode::CONFLICT, msg)
            }
            AppError::Internal => {
                tracing::error!("Internal server error occurred");
                (StatusCode::INTERNAL_SERVER_ERROR, "Internal server error".to_string())
            }
        };

        let body = Json(json!({
            "error": message
        }));

        (status, body).into_response()
    }
}
