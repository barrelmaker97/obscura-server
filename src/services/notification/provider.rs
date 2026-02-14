use async_trait::async_trait;

#[async_trait]
pub trait PushProvider: Send + Sync + std::fmt::Debug {
    /// Sends a push notification to a specific device token.
    async fn send_push(&self, token: &str) -> anyhow::Result<()>;
}
