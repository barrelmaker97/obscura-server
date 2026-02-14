use async_trait::async_trait;
use thiserror::Error;

#[derive(Error, Debug)]
pub enum PushError {
    #[error("Token is no longer registered")]
    Unregistered,
    #[error("Rate limit exceeded")]
    QuotaExceeded,
    #[error("External service error: {0}")]
    Other(#[from] anyhow::Error),
}

#[async_trait]
pub trait PushProvider: Send + Sync + std::fmt::Debug {
    /// Sends a push notification to a specific device token.
    /// 
    /// # Errors
    /// Returns `PushError::Unregistered` if the token is invalid and should be deleted.
    async fn send_push(&self, token: &str) -> Result<(), PushError>;
}
