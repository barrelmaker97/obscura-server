use crate::services::notification::provider::{PushError, PushProvider};
use async_trait::async_trait;

#[derive(Debug, Default)]
pub struct FcmPushProvider;

#[async_trait]
impl PushProvider for FcmPushProvider {
    async fn send_push(&self, token: &str) -> Result<(), PushError> {
        tracing::info!(token = %token, "STUB: Sending FCM push notification");
        Ok(())
    }
}
